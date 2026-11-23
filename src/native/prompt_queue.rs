//! Persistent prompt lists and fail-closed dispatch authorization.
use super::store;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const MAX_FILE: u64 = 2 * 1024 * 1024;
const MAX_SESSIONS: usize = 200;
pub const MAX_ITEMS: usize = 32;
pub const MAX_PROMPT: usize = 2048;
const IDLE_FRESH: u64 = 8;
const STOP_WAIT: u64 = 3 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Busy,
    Stopping,
    InputNeeded,
    Idle,
    Uncertain,
}

pub type TurnId = [u8; 16];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub id: TurnId,
    pub phase: Phase,
    pub at: u64,
}

impl Turn {
    pub(super) fn event(previous: Option<Self>, event: &str, at: u64) -> Option<Self> {
        if event == "UserPromptSubmit" {
            return Some(Self {
                id: *Uuid::new_v4().as_bytes(),
                phase: Phase::Busy,
                at,
            });
        }
        let previous = previous?;
        let phase = match event {
            "Stop" if previous.phase == Phase::Busy => Phase::Stopping,
            "idle_prompt" if previous.phase == Phase::Stopping => Phase::Idle,
            "idle_prompt" => return Some(previous),
            "PermissionRequest"
            | "PreToolUse"
            | "permission_prompt"
            | "elicitation_dialog"
            | "elicitation_url_dialog"
            | "agent_needs_input" => Phase::InputNeeded,
            _ => Phase::Uncertain,
        };
        Some(Self {
            phase,
            at,
            ..previous
        })
    }

    pub fn can_arm(self) -> bool {
        matches!(self.phase, Phase::Busy | Phase::Stopping)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dispatch {
    Wait,
    Pause,
    Send,
}

pub fn dispatch(
    running: bool,
    bracketed: bool,
    turn: Option<Turn>,
    already_sent: Option<TurnId>,
    now: u64,
) -> Dispatch {
    if !running {
        return Dispatch::Pause;
    }
    let Some(turn) = turn else {
        return Dispatch::Pause;
    };
    match turn.phase {
        Phase::Busy => Dispatch::Wait,
        Phase::Stopping if now.saturating_sub(turn.at) <= STOP_WAIT => Dispatch::Wait,
        Phase::Idle if already_sent == Some(turn.id) => Dispatch::Wait,
        Phase::Idle if bracketed && now >= turn.at && now - turn.at <= IDLE_FRESH => Dispatch::Send,
        _ => Dispatch::Pause,
    }
}

/// Intent belongs to this app instance. Loading a persisted queue never restores an arm.
#[derive(Clone, Copy, Debug, Default)]
pub enum Arming {
    #[default]
    Paused,
    Armed {
        input: u64,
        local_input: u64,
        turn: TurnId,
        sent: bool,
    },
}

impl Arming {
    pub fn arm(input: u64, local_input: u64, turn: Turn) -> Self {
        if !turn.can_arm() {
            return Self::Paused;
        }
        Self::Armed {
            input,
            local_input,
            turn: turn.id,
            sent: false,
        }
    }

    pub fn active(self) -> bool {
        matches!(self, Self::Armed { .. })
    }
}

#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queue {
    pub revision: u64,
    pub entries: Vec<String>,
    /// The previous attempt may have reached the terminal. Editing the first item or explicitly
    /// arming a later turn is a user decision. Reopening the app never retries it.
    pub uncertain: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Edit {
    Add { text: String },
    Replace { index: usize, text: String },
    Remove { index: usize },
    Move { index: usize, up: bool },
}

impl Queue {
    fn apply(&mut self, edit: Edit) -> bool {
        match edit {
            Edit::Add { text } if self.entries.len() < MAX_ITEMS && valid(&text) => {
                self.entries.push(text);
            }
            Edit::Replace { index, text } if index < self.entries.len() && valid(&text) => {
                self.entries[index] = text;
                if index == 0 {
                    self.uncertain = false;
                }
            }
            Edit::Remove { index } if index < self.entries.len() => {
                self.entries.remove(index);
                if index == 0 {
                    self.uncertain = false;
                }
            }
            Edit::Move { index, up } if index < self.entries.len() => {
                let other = if up {
                    index.checked_sub(1)
                } else {
                    index.checked_add(1)
                };
                let Some(other) = other.filter(|i| *i < self.entries.len()) else {
                    return false;
                };
                if self.uncertain && (index == 0 || other == 0) {
                    return false;
                }
                self.entries.swap(index, other);
            }
            _ => return false,
        }
        true
    }
}

fn valid(text: &str) -> bool {
    text.len() <= MAX_PROMPT && !strip_controls(text).trim().is_empty()
}

/// Remove C0/C1 sequences (including multi-byte ANSI/OSC/DCS), normalizing pasted line ends.
fn strip_controls(text: &str) -> String {
    use Escape::*;
    enum Escape {
        Text,
        Start,
        Intermediate,
        Csi,
        ControlString(bool),
        EndString(bool),
    }
    let mut state = Text;
    let mut clean = String::new();
    for ch in text.replace("\r\n", "\n").replace('\r', "\n").chars() {
        state = match (state, ch) {
            (Text, '\x1b') => Start,
            (Text, '\u{9b}') => Csi,
            (Text, '\u{9d}') => ControlString(true),
            (Text, '\u{90}' | '\u{98}' | '\u{9e}' | '\u{9f}') => ControlString(false),
            (Text, '\n' | '\t') => {
                clean.push(ch);
                Text
            }
            (Text, ch) => {
                if !ch.is_control() {
                    clean.push(ch);
                }
                Text
            }
            (Start, '[') => Csi,
            (Start, ']') => ControlString(true),
            (Start, 'P' | 'X' | '^' | '_') => ControlString(false),
            (Start | Intermediate, '\x1b') => Start,
            (Start | Intermediate, ' '..='/') => Intermediate,
            (Start | Intermediate, _) => Text,
            (Csi, '\x1b') => Start,
            (Csi, '@'..='~') => Text,
            (Csi, _) => Csi,
            (ControlString(true) | EndString(true), '\x07') => Text,
            (ControlString(_) | EndString(_), '\u{9c}') | (EndString(_), '\\') => Text,
            (ControlString(osc) | EndString(osc), '\x1b') => EndString(osc),
            (ControlString(osc) | EndString(osc), _) => ControlString(osc),
        };
    }
    clean
}

/// One paste frame and precisely one deliberate submit byte. A failed or partial PTY write
/// is ambiguous, so the queue will hold this item instead of retrying it.
pub(super) fn input(text: &str) -> Option<Vec<u8>> {
    if text.len() > MAX_PROMPT {
        return None;
    }
    let clean = strip_controls(text);
    (!clean.trim().is_empty()).then(|| format!("\x1b[200~{clean}\x1b[201~\r").into_bytes())
}

pub(super) struct Book {
    path: PathBuf,
    data: Option<BTreeMap<String, Queue>>,
}

impl Book {
    pub(super) fn open(base: &Path) -> Self {
        let path = base.join("prompt-queues.json");
        let data = match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Some(BTreeMap::new()),
            Ok(_) => store::read_bounded(&path, MAX_FILE)
                .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, Queue>>(&bytes).ok())
                .filter(|items| {
                    items.len() <= MAX_SESSIONS
                        && items.iter().all(|(id, queue)| {
                            id.parse::<Uuid>().is_ok()
                                && queue.entries.len() <= MAX_ITEMS
                                && queue.entries.iter().all(|text| valid(text))
                        })
                }),
            _ => None,
        };
        Self { path, data }
    }

    pub(super) fn get(&self, id: &str) -> Option<Queue> {
        Some(self.data.as_ref()?.get(id).cloned().unwrap_or_default())
    }

    pub(super) fn summary(&self, id: &str) -> Option<(u64, usize)> {
        Some(
            self.data
                .as_ref()?
                .get(id)
                .map_or((0, 0), |q| (q.revision, q.entries.len())),
        )
    }

    fn update(
        &mut self,
        id: &str,
        revision: u64,
        change: impl FnOnce(&mut Queue) -> bool,
    ) -> Option<Queue> {
        let mut next = self.data.clone()?;
        let queue = next.entry(id.to_owned()).or_default();
        if revision != queue.revision || !change(queue) {
            return None;
        }
        queue.revision = revision.checked_add(1)?;
        let result = queue.clone();
        self.commit(next)?;
        Some(result)
    }

    pub(super) fn edit(&mut self, id: &str, revision: u64, edit: Edit) -> Option<Queue> {
        self.update(id, revision, |queue| queue.apply(edit))
    }

    pub(super) fn forget(&mut self, id: &str) {
        if let Some(mut next) = self.data.clone()
            && next.remove(id).is_some()
        {
            let _ = self.commit(next);
        }
    }

    fn commit(&mut self, next: BTreeMap<String, Queue>) -> Option<()> {
        let bytes = serde_json::to_vec(&next).ok()?;
        if next.len() > MAX_SESSIONS || bytes.len() as u64 > MAX_FILE {
            return None;
        }
        match store::write_atomic_staged(&self.path, &bytes) {
            Ok(()) => {
                self.data = Some(next);
                Some(())
            }
            Err(store::AtomicWriteError::AfterRename(_)) => {
                self.data = Some(next);
                None
            }
            Err(store::AtomicWriteError::BeforeRename(_)) => None,
        }
    }

    pub(super) fn send(
        &mut self,
        id: &str,
        revision: u64,
        write: impl FnOnce(Vec<u8>) -> bool,
    ) -> Option<Queue> {
        let queue = self.get(id)?;
        if queue.revision != revision || queue.uncertain {
            return None;
        }
        let input = input(queue.entries.first()?)?;
        let pending = self.update(id, revision, |queue| {
            queue.uncertain = true;
            true
        })?;
        if !write(input) {
            return None;
        }
        self.update(id, pending.revision, |queue| {
            queue.entries.remove(0);
            queue.uncertain = false;
            true
        })
    }
}

#[cfg(test)]
mod tests;
