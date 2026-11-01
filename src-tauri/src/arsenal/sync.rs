//! Arsenal synchronization between the owner's computers.
//!
//! Each computer keeps its own Arsenal database. What they share is a folder that a file
//! synchronizer (Syncthing) replicates: `v1/devices/<device>/r-<seq>.json`, an append-only journal
//! that only its own device writes, and `v1/blobs/<sha256>`, file contents addressed by their digest.
//! Nothing in the folder is ever rewritten by two computers, so the synchronizer has nothing to
//! conflict over; a database file never travels.
//!
//! A record says "this object is now this", and names the version it was made from (`base`).
//! The versions of an object that no record replaced are its heads. One head is the object. Several
//! heads with different contents are a conflict, kept, never decided by losing one: the order below
//! picks the head the object shows, the others stay (a capability keeps them as older revisions,
//! a preset or selection gets a copy named "(conflict)"), and a merge record ends the conflict.
//! A deletion never wins over an edit. The same set of records gives every computer the same result.
//!
//! What arrives is checked as an import from disk is: the store's own validation, and no file that
//! is a credential by its name or its content. Nothing received is run; receiving changes no
//! provider's files.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::store::{
    self, Error as StoreError, Item, NewCapability, PresetInput, SelectionInput, Store,
    SyncCapability,
};
use crate::agentconfig::catalogue::{MAX_FILE_BYTES, MAX_FILES, MAX_TREE_BYTES, unsafe_file};

pub const SCHEMA: &str = "wyss.arsenal.sync.v1";
/// A record lists file digests, never contents.
const MAX_RECORD_BYTES: u64 = 256 * 1024;
const MAX_DEVICES: usize = 16;
/// Entries read from one folder of the shared folder, and records and bytes read in all: past
/// them a run fails closed and applies nothing, rather than reading without end.
const MAX_DEVICE_ENTRIES: usize = 64;
const MAX_DIR_ENTRIES: usize = 60_000;
const MAX_RECORDS: usize = 50_000;
const MAX_RECORDS_BYTES: u64 = 128 * 1024 * 1024;
/// File contents read in one run; what does not fit is applied by the next run.
const MAX_BLOB_BYTES_PER_RUN: u64 = 256 * 1024 * 1024;
const MORE_NEXT_RUN: &str = "more to apply; continues in the next run";
const MAX_MERGED: usize = 16;
const MAX_NOTES: usize = 50;
/// Rounds of apply-and-publish in one reconciliation; a conflict needs at most two.
const ROUNDS: usize = 4;

// ---------------------------------------------------------------------------------------------
// The shared format

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema: String,
    pub device: String,
    pub host: String,
    pub seq: u64,
    pub at: i64,
    /// `capability/<identity>`, `preset/<uuid>` or `selection/<uuid>`.
    pub object: String,
    /// The version this one was made from: `<device>:<seq>`.
    pub base: Option<String>,
    /// Other heads this version replaces: a merge.
    #[serde(default)]
    pub merged: Vec<String>,
    pub body: Body,
}

impl Record {
    pub fn id(&self) -> String {
        format!("{}:{}", self.device, self.seq)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Body {
    Capability(CapabilityBody),
    Preset(PresetBody),
    Selection(SelectionBody),
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityBody {
    pub provider: String,
    pub source: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub origin: String,
    pub tree_hash: String,
    pub files: Vec<FileRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetBody {
    pub name: String,
    pub description: String,
    pub items: Vec<SharedItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionBody {
    pub name: String,
    pub provider: String,
    pub items: Vec<SharedItem>,
    pub composites: Vec<String>,
}

/// A capability by identity, and a pinned revision by content: local ids and revision numbers
/// differ from one computer to the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedItem {
    pub identity: String,
    pub tree_hash: Option<String>,
}

/// What each computer has applied of the others, written only by its owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Applied {
    schema: String,
    device: String,
    host: String,
    at: i64,
    /// For each device, the highest sequence number whose records are all applied.
    seen: BTreeMap<String, u64>,
}

// ---------------------------------------------------------------------------------------------
// Status, as the window shows it

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    pub enabled: bool,
    pub folder: String,
    pub device: String,
    pub host: String,
    /// Bumped each time synchronization changed this computer's library.
    pub generation: u64,
    pub last_run: Option<i64>,
    pub last_success: Option<i64>,
    pub error: Option<String>,
    /// Records this computer published and that are waiting for no one.
    pub published: u64,
    pub pending: Vec<Note>,
    pub conflicts: Vec<Note>,
    pub rejected: Vec<Note>,
    /// Presets and selections deleted here because another computer deleted them, each with
    /// where its copy is kept.
    pub removed: Vec<Note>,
    pub peers: Vec<Peer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub at: i64,
    pub kind: String,
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Peer {
    pub device: String,
    pub host: String,
    /// When its last record or acknowledgment was written there.
    pub last_heard: Option<i64>,
    /// How many of this computer's records it has applied, of how many this computer published.
    pub applied_mine: u64,
    pub published_mine: u64,
}

// ---------------------------------------------------------------------------------------------
// Where things are

#[derive(Debug, Clone)]
pub struct Paths {
    /// The synchronized folder (`…/wyss-console-sync/arsenal`).
    pub shared: PathBuf,
    /// This computer's own bookkeeping, never synchronized.
    pub local: PathBuf,
}

impl Paths {
    /// The folders for this account, or `None` when synchronization is switched off with
    /// `WYSS_ARSENAL_SYNC=off` or the home directory is unknown.
    pub fn from_env() -> Option<Self> {
        if std::env::var("WYSS_ARSENAL_SYNC").is_ok_and(|value| value == "off") {
            return None;
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|home| home.is_absolute())?;
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/share"));
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".local/state"));
        Some(Self {
            shared: data.join("wyss-console-sync/arsenal"),
            local: state.join("wyss-console/arsenal-sync"),
        })
    }

    fn root(&self) -> PathBuf {
        self.shared.join("v1")
    }
    fn devices(&self) -> PathBuf {
        self.root().join("devices")
    }
    fn blobs(&self) -> PathBuf {
        self.root().join("blobs")
    }
    fn staging(&self) -> PathBuf {
        self.local.join("staging")
    }
}

// ---------------------------------------------------------------------------------------------
// This computer's bookkeeping

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Local {
    device: String,
    next_seq: u64,
    /// For each object, the version this computer's library was last brought to, and its content.
    objects: BTreeMap<String, Synced>,
    /// Record ids whose content was refused here, with why. They are not tried again.
    refused: BTreeMap<String, String>,
    conflicts: Vec<Note>,
    rejected: Vec<Note>,
    #[serde(default)]
    removed: Vec<Note>,
    generation: u64,
    last_success: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Synced {
    record: Option<String>,
    content: String,
}

const ABSENT: &str = "absent";
const DELETED: &str = "deleted";

// ---------------------------------------------------------------------------------------------
// Reconciliation

/// Brings the shared folder and this computer's library to the same state: what others published
/// is applied here, what changed here is published. Safe to run at any time and any number of
/// times; one that is interrupted leaves both sides consistent and the next one continues.
pub fn reconcile(store: &mut Store, paths: &Paths, host: &str) -> Result<Status, String> {
    for dir in [&paths.local, &paths.staging()] {
        make_dir(dir).map_err(|e| format!("folder: {e}"))?;
    }
    for parts in [["v1", "devices"], ["v1", "blobs"]] {
        Dir::open(paths, &parts, true)?;
    }
    let lock = File::create(paths.local.join("lock")).map_err(|e| format!("lock: {e}"))?;
    let _guard = nix::fcntl::Flock::lock(lock, nix::fcntl::FlockArg::LockExclusiveNonblock)
        .map_err(|_| "busy".to_owned())?;
    let mut local = load_local(paths)?;
    let host = clean_host(host);
    let journal =
        Dir::open(paths, &["v1", "devices", &local.device], true)?.ok_or("folder: unavailable")?;
    let mut budget = MAX_BLOB_BYTES_PER_RUN;
    let before = local.generation;
    let mut published = 0_u64;
    let mut deferred: BTreeMap<String, Note> = BTreeMap::new();
    for _ in 0..ROUNDS {
        let (records, refused_files) = read_records(paths)?;
        note_rejected(&mut local, &refused_files);
        let views = materialize(&records);
        deferred.clear();
        let mut wrote = apply_and_publish(
            store,
            paths,
            &journal,
            &host,
            &mut local,
            &records,
            &views,
            &mut deferred,
            &mut budget,
        )?;
        let (records, _) = read_records(paths)?;
        let views = materialize(&records);
        wrote += resolve_conflicts(paths, &journal, &host, &mut local, &records, &views)?;
        published += wrote;
        save_local(paths, &local)?;
        if wrote == 0 {
            break;
        }
    }
    let (records, _) = read_records(paths)?;
    write_applied(paths, &journal, &host, &local, &records, &deferred)?;
    if local.generation != before || published > 0 {
        save_local(paths, &local)?;
    }
    local.last_success = Some(now());
    save_local(paths, &local)?;
    Ok(status(
        paths,
        &host,
        &local,
        &records,
        deferred.into_values().collect(),
    ))
}

/// The current status without changing anything, for a window that opens before the first run.
pub fn read_status(paths: &Paths, host: &str) -> Status {
    let host = clean_host(host);
    match load_local_existing(paths) {
        Some(local) => {
            let mut status = match read_records(paths) {
                Ok((records, _)) => status(paths, &host, &local, &records, Vec::new()),
                Err(error) => Status {
                    error: Some(error),
                    ..status(paths, &host, &local, &[], Vec::new())
                },
            };
            status.last_run = None;
            status
        }
        None => Status {
            enabled: true,
            folder: display(&paths.shared),
            host,
            ..Status::default()
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_and_publish(
    store: &mut Store,
    paths: &Paths,
    journal: &Dir,
    host: &str,
    local: &mut Local,
    records: &[Record],
    views: &BTreeMap<String, View>,
    deferred: &mut BTreeMap<String, Note>,
    budget: &mut u64,
) -> Result<u64, String> {
    let mut wrote = 0;
    // Capabilities first: presets and selections refer to them.
    let mine = store.sync_capabilities().map_err(store_text)?;
    let by_identity: BTreeMap<String, SyncCapability> = mine
        .into_iter()
        .map(|capability| (capability.identity.clone(), capability))
        .collect();
    let mut keys: BTreeSet<String> = by_identity
        .keys()
        .map(|identity| format!("capability/{identity}"))
        .collect();
    keys.extend(
        views
            .keys()
            .filter(|key| key.starts_with("capability/"))
            .cloned(),
    );
    for key in &keys {
        let identity = &key["capability/".len()..];
        let here = by_identity.get(identity);
        let content = here.map_or(ABSENT.to_owned(), |c| format!("tree:{}", current_tree(c)));
        let synced = local.objects.get(key).cloned();
        let synced_content = synced.as_ref().map_or(ABSENT, |s| s.content.as_str());
        let view = views.get(key);
        // Already the same here as what was published (imported on both computers): adopt it.
        if synced.is_none()
            && let Some(view) = view
            && content_of(&records[view.chosen].body) == content
        {
            local.objects.insert(
                key.clone(),
                Synced {
                    record: Some(records[view.chosen].id()),
                    content,
                },
            );
            continue;
        }
        let dirty = content != synced_content && here.is_some();
        if dirty {
            let capability = here.expect("dirty means present");
            let base = synced.as_ref().and_then(|s| s.record.clone());
            let id = if base.is_none() && view.is_none() {
                publish_history(store, paths, journal, host, local, key, capability)?
            } else {
                let current = revision_of(capability, capability.current);
                publish_capability(
                    store,
                    paths,
                    journal,
                    host,
                    local,
                    key,
                    capability,
                    current,
                    base,
                    Vec::new(),
                )?
            };
            wrote += 1;
            local.objects.insert(
                key.clone(),
                Synced {
                    record: Some(id),
                    content,
                },
            );
            continue;
        }
        let Some(view) = view else { continue };
        let chosen = &records[view.chosen];
        if synced.as_ref().and_then(|s| s.record.as_deref()) == Some(chosen.id().as_str()) {
            continue;
        }
        if local.refused.contains_key(&chosen.id()) {
            continue;
        }
        match apply_capability(store, paths, records, view, budget) {
            Ok(tree) => {
                local.generation += 1;
                local.objects.insert(
                    key.clone(),
                    Synced {
                        record: Some(chosen.id()),
                        content: format!("tree:{tree}"),
                    },
                );
            }
            Err(Applying::Wait(why)) => {
                deferred.insert(key.clone(), note("capability", &object_name(chosen), why));
            }
            Err(Applying::Refuse(why)) => {
                local.refused.insert(chosen.id(), why.clone());
                push_note(
                    &mut local.rejected,
                    note(
                        "capability",
                        &object_name(chosen),
                        &format!("{why} (from {})", chosen.host),
                    ),
                );
            }
            Err(Applying::Fail(why)) => return Err(why),
        }
    }

    // Presets and selections, against the library as it is now.
    let capabilities = store.sync_capabilities().map_err(store_text)?;
    let ids: BTreeMap<String, &SyncCapability> =
        capabilities.iter().map(|c| (c.id.clone(), c)).collect();
    let identities: BTreeMap<String, &SyncCapability> = capabilities
        .iter()
        .map(|c| (c.identity.clone(), c))
        .collect();
    let mut objects: BTreeMap<String, Body> = BTreeMap::new();
    for preset in store.presets().map_err(store_text)? {
        if let Some(items) = shared_items(&preset.items, &ids) {
            objects.insert(
                format!("preset/{}", preset.id),
                Body::Preset(PresetBody {
                    name: preset.name,
                    description: preset.description,
                    items,
                }),
            );
        }
    }
    for selection in store.selections().map_err(store_text)? {
        if let Some(items) = shared_items(&selection.items, &ids) {
            objects.insert(
                format!("selection/{}", selection.id),
                Body::Selection(SelectionBody {
                    name: selection.name,
                    provider: selection.provider,
                    items,
                    composites: selection.composites,
                }),
            );
        }
    }
    let mut keys: BTreeSet<String> = objects.keys().cloned().collect();
    keys.extend(
        views
            .keys()
            .filter(|key| key.starts_with("preset/") || key.starts_with("selection/"))
            .cloned(),
    );
    keys.extend(
        local
            .objects
            .keys()
            .filter(|key| key.starts_with("preset/") || key.starts_with("selection/"))
            .cloned(),
    );
    for key in &keys {
        let here = objects.get(key);
        let content = here.map(content_of);
        let synced = local.objects.get(key).cloned();
        if synced.is_none()
            && let (Some(view), Some(content)) = (views.get(key), &content)
            && content_of(&records[view.chosen].body) == *content
        {
            local.objects.insert(
                key.clone(),
                Synced {
                    record: Some(records[view.chosen].id()),
                    content: content.clone(),
                },
            );
            continue;
        }
        let synced_content = synced.as_ref().map(|s| s.content.clone());
        let edited = match (&content, &synced_content) {
            (Some(content), Some(synced)) => content != synced,
            (Some(_), None) => true,
            // Gone here after being here: deleted here.
            (None, Some(synced)) => synced != DELETED && synced != ABSENT,
            (None, None) => false,
        };
        let view = views.get(key);
        if edited {
            let base = synced.as_ref().and_then(|s| s.record.clone());
            let body = here.cloned().unwrap_or(Body::Deleted);
            let id = publish(
                paths,
                journal,
                host,
                local,
                key,
                base,
                Vec::new(),
                body.clone(),
            )?;
            wrote += 1;
            local.objects.insert(
                key.clone(),
                Synced {
                    record: Some(id),
                    content: content.unwrap_or_else(|| DELETED.to_owned()),
                },
            );
            continue;
        }
        let Some(view) = view else { continue };
        let chosen = &records[view.chosen];
        if synced.as_ref().and_then(|s| s.record.as_deref()) == Some(chosen.id().as_str()) {
            continue;
        }
        match &chosen.body {
            Body::Deleted => {
                let id = &key[key.find('/').map_or(0, |i| i + 1)..];
                if let Some(body) = here {
                    // What another computer deletes stays recoverable here.
                    let kept = keep_deleted(paths, key, body)?;
                    push_note(
                        &mut local.removed,
                        note(
                            kind_of(key),
                            &body_name(body),
                            &format!("deleted on {}; a copy is kept in {kept}", chosen.host),
                        ),
                    );
                    let removed = if key.starts_with("preset/") {
                        store.delete_preset(id)
                    } else {
                        store.delete_selection(id)
                    };
                    match removed {
                        Ok(()) | Err(StoreError::NotFound) => {}
                        Err(error) => return Err(store_text(error)),
                    }
                    local.generation += 1;
                }
                local.objects.insert(
                    key.clone(),
                    Synced {
                        record: Some(chosen.id()),
                        content: DELETED.to_owned(),
                    },
                );
            }
            body => {
                let Some(items) = local_items(store, body_items(body), &identities)? else {
                    deferred.insert(
                        key.clone(),
                        note(
                            kind_of(key),
                            &object_name(chosen),
                            "waiting for a capability it uses",
                        ),
                    );
                    continue;
                };
                let id = &key[key.find('/').map_or(0, |i| i + 1)..];
                let written = match body {
                    Body::Preset(preset) => store.put_preset(
                        id,
                        &PresetInput {
                            id: None,
                            name: preset.name.clone(),
                            description: preset.description.clone(),
                            items,
                        },
                    ),
                    Body::Selection(selection) => store.put_selection(
                        id,
                        &SelectionInput {
                            id: None,
                            name: selection.name.clone(),
                            provider: selection.provider.clone(),
                            items,
                            composites: selection.composites.clone(),
                        },
                    ),
                    _ => Err(StoreError::Invalid),
                };
                match written {
                    Ok(()) => {
                        local.generation += 1;
                        local.objects.insert(
                            key.clone(),
                            Synced {
                                record: Some(chosen.id()),
                                content: content_of(body),
                            },
                        );
                    }
                    Err(StoreError::Invalid) => {
                        local.refused.insert(chosen.id(), "invalid".into());
                        push_note(
                            &mut local.rejected,
                            note(kind_of(key), &object_name(chosen), "invalid"),
                        );
                    }
                    Err(error) => return Err(store_text(error)),
                }
            }
        }
    }
    Ok(wrote)
}

/// Ends each conflict: the chosen head stays, every other head is kept (as an older revision of a
/// capability, as a copy of a preset or selection), and a merge record makes one head again.
fn resolve_conflicts(
    paths: &Paths,
    journal: &Dir,
    host: &str,
    local: &mut Local,
    records: &[Record],
    views: &BTreeMap<String, View>,
) -> Result<u64, String> {
    let mut wrote = 0;
    let objects: BTreeSet<&String> = records.iter().map(|r| &r.object).collect();
    for (key, view) in views {
        if !view.conflict {
            continue;
        }
        let chosen = &records[view.chosen];
        let others: Vec<&Record> = view
            .heads
            .iter()
            .map(|&i| &records[i])
            .filter(|r| r.id() != chosen.id())
            .collect();
        for loser in &others {
            if content_of(&loser.body) == content_of(&chosen.body) || loser.body == Body::Deleted {
                continue;
            }
            let copy = match &loser.body {
                Body::Preset(preset) => Some(Body::Preset(PresetBody {
                    name: conflict_name(&preset.name, &loser.host),
                    ..preset.clone()
                })),
                Body::Selection(selection) => Some(Body::Selection(SelectionBody {
                    name: conflict_name(&selection.name, &loser.host),
                    ..selection.clone()
                })),
                _ => None,
            };
            let kept = if let Some(copy) = copy {
                let kind = kind_of(key);
                let copy_key =
                    format!("{kind}/{}", derived_uuid(&format!("{key}\0{}", loser.id())));
                if !objects.contains(&copy_key) {
                    publish(
                        paths,
                        journal,
                        host,
                        local,
                        &copy_key,
                        None,
                        Vec::new(),
                        copy,
                    )?;
                    wrote += 1;
                }
                format!(
                    "the version from {} is kept as a copy named \"{}\"",
                    loser.host,
                    conflict_name(&object_name(loser), &loser.host)
                )
            } else {
                format!(
                    "the version from {} is kept as an older revision",
                    loser.host
                )
            };
            push_note(
                &mut local.conflicts,
                note(
                    kind_of(key),
                    &object_name(chosen),
                    &format!(
                        "changed on two computers; {} shows the version from {}, {kept}",
                        "this library", chosen.host
                    ),
                ),
            );
        }
        let merged: Vec<String> = others.iter().map(|r| r.id()).take(MAX_MERGED).collect();
        publish(
            paths,
            journal,
            host,
            local,
            key,
            Some(chosen.id()),
            merged,
            chosen.body.clone(),
        )?;
        wrote += 1;
    }
    Ok(wrote)
}

// ---------------------------------------------------------------------------------------------
// Materializing: every computer derives the same objects from the same records

#[derive(Debug, Clone)]
struct View {
    chosen: usize,
    heads: Vec<usize>,
    conflict: bool,
    /// Every record of the object, oldest first by rank.
    all: Vec<usize>,
}

fn rank(record: &Record) -> (bool, i64, &str, u64) {
    (
        record.body != Body::Deleted,
        record.at,
        record.device.as_str(),
        record.seq,
    )
}

fn materialize(records: &[Record]) -> BTreeMap<String, View> {
    let mut by_object: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        by_object.entry(&record.object).or_default().push(index);
    }
    let mut views = BTreeMap::new();
    for (object, mut indices) in by_object {
        indices.sort_by(|&a, &b| rank(&records[a]).cmp(&rank(&records[b])));
        let superseded: BTreeSet<String> = indices
            .iter()
            .flat_map(|&i| {
                let r = &records[i];
                r.base.iter().cloned().chain(r.merged.iter().cloned())
            })
            .collect();
        let mut heads: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&i| !superseded.contains(&records[i].id()))
            .collect();
        if heads.is_empty() {
            // Only a cycle of references gets here; the newest record decides.
            heads.push(*indices.last().expect("an object has a record"));
        }
        let chosen = *heads
            .iter()
            .max_by(|&&a, &&b| rank(&records[a]).cmp(&rank(&records[b])))
            .expect("at least one head");
        let contents: BTreeSet<String> = heads
            .iter()
            .map(|&i| content_of(&records[i].body))
            .collect();
        views.insert(
            object.to_owned(),
            View {
                chosen,
                heads,
                conflict: contents.len() > 1,
                all: indices,
            },
        );
    }
    views
}

/// What an object holds, for comparing versions. A capability is its content's digest.
fn content_of(body: &Body) -> String {
    match body {
        Body::Capability(capability) => format!("tree:{}", capability.tree_hash),
        Body::Deleted => DELETED.to_owned(),
        other => store::digest(&serde_json::to_vec(other).expect("a body serializes")),
    }
}

// ---------------------------------------------------------------------------------------------
// Applying a capability

enum Applying {
    /// Something it needs has not arrived yet; the next run tries again.
    Wait(&'static str),
    /// Its content may not be stored here.
    Refuse(String),
    Fail(String),
}

/// Stores every revision the records hold, oldest first, and makes the chosen one current.
fn apply_capability(
    store: &mut Store,
    paths: &Paths,
    records: &[Record],
    view: &View,
    budget: &mut u64,
) -> Result<String, Applying> {
    let blobs = match Dir::open(paths, &["v1", "blobs"], false) {
        Ok(Some(blobs)) => blobs,
        Ok(None) => return Err(Applying::Wait("file contents not received yet")),
        Err(why) => return Err(Applying::Fail(why)),
    };
    let chosen = &records[view.chosen];
    let mut order: Vec<&Record> = view
        .all
        .iter()
        .map(|&i| &records[i])
        .filter(|r| matches!(r.body, Body::Capability(_)) && r.id() != chosen.id())
        .collect();
    order.push(chosen);
    let mut done: BTreeSet<String> = BTreeSet::new();
    let last = order.len() - 1;
    for (i, record) in order.into_iter().enumerate() {
        let Body::Capability(body) = &record.body else {
            return Err(Applying::Refuse("not a capability".into()));
        };
        let current = i == last;
        if !current && !done.insert(body.tree_hash.clone()) {
            continue;
        }
        let size: u64 = body.files.iter().map(|f| f.size).sum();
        if size > *budget {
            return Err(Applying::Wait(MORE_NEXT_RUN));
        }
        *budget -= size;
        let new = assemble(&blobs, &record.object["capability/".len()..], body)?;
        match store.import_revision(&new, current) {
            Ok(_) => {}
            Err(StoreError::Invalid) => return Err(Applying::Refuse("invalid capability".into())),
            Err(StoreError::Busy) => return Err(Applying::Wait("library busy")),
            Err(error) => return Err(Applying::Fail(store_text(error))),
        }
    }
    let Body::Capability(body) = &chosen.body else {
        unreachable!("the chosen record is a capability")
    };
    Ok(body.tree_hash.clone())
}

/// The capability a record describes, with its files read back from the shared blobs and every
/// file checked as an import from disk would check it.
fn assemble(blobs: &Dir, identity: &str, body: &CapabilityBody) -> Result<NewCapability, Applying> {
    let mut files = Vec::with_capacity(body.files.len());
    let mut total = 0_u64;
    for file in &body.files {
        let bytes = match read_blob(blobs, &file.sha256, file.size) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Err(Applying::Wait("file contents not received yet")),
            Err(why) => return Err(Applying::Refuse(why)),
        };
        if let Some(reason) = unsafe_file(&file.path, &bytes) {
            return Err(Applying::Refuse(format!("{} ({reason})", file.path)));
        }
        total += file.size;
        files.push((file.path.clone(), bytes));
    }
    if total > MAX_TREE_BYTES as u64 || store::tree_digest(&files) != body.tree_hash {
        return Err(Applying::Refuse("content does not match its digest".into()));
    }
    Ok(NewCapability {
        identity: identity.to_owned(),
        provider: body.provider.clone(),
        source: body.source.clone(),
        kind: body.kind.clone(),
        name: body.name.clone(),
        description: body.description.clone(),
        origin: body.origin.clone(),
        files,
    })
}

fn read_blob(blobs: &Dir, hash: &str, size: u64) -> Result<Option<Vec<u8>>, String> {
    if !hex64(hash) {
        return Err("a shared file has a bad name".into());
    }
    let file = match blobs.file(hash) {
        Ok(Some(file)) => file,
        Ok(None) => return Ok(None),
        Err(why) => return Err(format!("blob: {why}")),
    };
    let meta = file.metadata().map_err(|e| format!("blob: {e}"))?;
    if meta.len() != size {
        // A blob still being written by the synchronizer has the wrong size for now.
        return Ok(None);
    }
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(size + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("blob: {e}"))?;
    if store::digest(&bytes) != hash {
        return Err("a shared file does not match its digest".into());
    }
    Ok(Some(bytes))
}

// ---------------------------------------------------------------------------------------------
// Publishing

fn publish_history(
    store: &Store,
    paths: &Paths,
    journal: &Dir,
    host: &str,
    local: &mut Local,
    key: &str,
    capability: &SyncCapability,
) -> Result<String, String> {
    // Older revisions first, the current one last, so that the current one is the head.
    let mut order: Vec<&store::SyncRevision> = capability
        .revisions
        .iter()
        .filter(|r| r.number != capability.current)
        .collect();
    order.push(revision_of(capability, capability.current));
    let mut base = None;
    for revision in order {
        let id = publish_capability(
            store,
            paths,
            journal,
            host,
            local,
            key,
            capability,
            revision,
            base.clone(),
            Vec::new(),
        )?;
        base = Some(id);
    }
    Ok(base.expect("a capability has a revision"))
}

#[allow(clippy::too_many_arguments)]
fn publish_capability(
    store: &Store,
    paths: &Paths,
    journal: &Dir,
    host: &str,
    local: &mut Local,
    key: &str,
    capability: &SyncCapability,
    revision: &store::SyncRevision,
    base: Option<String>,
    merged: Vec<String>,
) -> Result<String, String> {
    let content = store
        .revision_content(&capability.id, revision.number)
        .map_err(store_text)?;
    let blobs = Dir::open(paths, &["v1", "blobs"], true)?.ok_or("folder: unavailable")?;
    let mut files = Vec::with_capacity(content.len());
    for (path, bytes) in &content {
        let hash = store::digest(bytes);
        write_blob(paths, &blobs, &hash, bytes)?;
        files.push(FileRef {
            path: path.clone(),
            sha256: hash,
            size: bytes.len() as u64,
        });
    }
    let body = Body::Capability(CapabilityBody {
        provider: capability.provider.clone(),
        source: capability.source.clone(),
        kind: capability.kind.clone(),
        name: capability.name.clone(),
        description: revision.description.clone(),
        origin: revision.origin.clone(),
        tree_hash: revision.tree_hash.clone(),
        files,
    });
    publish(paths, journal, host, local, key, base, merged, body)
}

/// Writes one record into this device's journal, whole or not at all.
#[allow(clippy::too_many_arguments)]
fn publish(
    paths: &Paths,
    journal: &Dir,
    host: &str,
    local: &mut Local,
    key: &str,
    base: Option<String>,
    merged: Vec<String>,
    body: Body,
) -> Result<String, String> {
    // A journal that already holds later records (bookkeeping restored from a backup) continues after them.
    let seq = local.next_seq.max(last_seq(journal)? + 1).max(1);
    let record = Record {
        schema: SCHEMA.to_owned(),
        device: local.device.clone(),
        host: host.to_owned(),
        seq,
        at: now(),
        object: key.to_owned(),
        base,
        merged,
        body,
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    write_new(paths, journal, &record_name(seq), &bytes, false)?;
    local.next_seq = seq + 1;
    Ok(record.id())
}

fn write_blob(paths: &Paths, blobs: &Dir, hash: &str, bytes: &[u8]) -> Result<(), String> {
    if fs::symlink_metadata(blobs.at(hash)).is_ok() {
        return Ok(());
    }
    match write_new(paths, blobs, hash, bytes, false) {
        // Named by its content: one already there is the same file.
        Err(error) if error.contains("exists") => Ok(()),
        other => other,
    }
}

/// Writes a file in the private staging folder and renames it into `dir`, so the synchronizer
/// never sees part of it. The staging folder is on the same file system as the shared one. The
/// rename is relative to the folder as opened, so a link planted in the shared folder cannot
/// redirect it; a file already there is replaced only when `replace` says so.
fn write_new(
    paths: &Paths,
    dir: &Dir,
    name: &str,
    bytes: &[u8],
    replace: bool,
) -> Result<(), String> {
    let temp = paths
        .staging()
        .join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| format!("write: {e}"))?;
        file.write_all(bytes).map_err(|e| format!("write: {e}"))?;
        file.sync_all().map_err(|e| format!("write: {e}"))?;
    }
    let flags = if replace {
        nix::fcntl::RenameFlags::empty()
    } else {
        nix::fcntl::RenameFlags::RENAME_NOREPLACE
    };
    if let Err(error) = nix::fcntl::renameat2(
        None,
        temp.as_path(),
        Some(dir.file.as_raw_fd()),
        Path::new(name),
        flags,
    ) {
        let _ = fs::remove_file(&temp);
        return Err(match error {
            nix::errno::Errno::EEXIST => format!("publish: {name} exists"),
            other => format!("publish: {other}"),
        });
    }
    let _ = dir.file.sync_all();
    Ok(())
}

fn write_applied(
    paths: &Paths,
    journal: &Dir,
    host: &str,
    local: &Local,
    records: &[Record],
    deferred: &BTreeMap<String, Note>,
) -> Result<(), String> {
    let mut seen: BTreeMap<String, u64> = BTreeMap::new();
    let mut blocked: BTreeMap<&str, u64> = BTreeMap::new();
    for record in records {
        if deferred.contains_key(&record.object) {
            let lowest = blocked.entry(record.device.as_str()).or_insert(u64::MAX);
            *lowest = (*lowest).min(record.seq);
        }
    }
    for record in records {
        let limit = blocked
            .get(record.device.as_str())
            .copied()
            .unwrap_or(u64::MAX);
        if record.seq < limit {
            let entry = seen.entry(record.device.clone()).or_insert(0);
            *entry = (*entry).max(record.seq);
        }
    }
    let previous = read_applied(journal);
    if previous
        .as_ref()
        .is_some_and(|p| p.seen == seen && p.host == host)
    {
        return Ok(());
    }
    let applied = Applied {
        schema: SCHEMA.to_owned(),
        device: local.device.clone(),
        host: host.to_owned(),
        at: now(),
        seen,
    };
    let bytes = serde_json::to_vec_pretty(&applied).map_err(|e| e.to_string())?;
    write_new(paths, journal, "applied.json", &bytes, true)
}

/// A device's acknowledgments, if its folder holds a plain, small, well-formed `applied.json`.
fn read_applied(dir: &Dir) -> Option<Applied> {
    let file = dir.file("applied.json").ok()??;
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 64 * 1024 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

// ---------------------------------------------------------------------------------------------
// Reading the shared folder

/// Every valid record of every device, and the files that were refused with why. A shared folder
/// larger than this version reads is an error: nothing is applied from part of it.
type Shared = (Vec<Record>, Vec<(String, String)>);
fn read_records(paths: &Paths) -> Result<Shared, String> {
    let mut records = Vec::new();
    let mut refused = Vec::new();
    let Some(devices) = Dir::open(paths, &["v1", "devices"], false)? else {
        return Ok((records, refused));
    };
    let mut names: Vec<String> = devices
        .names(MAX_DEVICE_ENTRIES)?
        .into_iter()
        .filter(|name| device_ok(name))
        .collect();
    names.sort();
    names.truncate(MAX_DEVICES);
    let mut bytes = 0_u64;
    for device in names {
        let dir = match devices.child(&device, false) {
            Ok(Some(dir)) => dir,
            Ok(None) => continue,
            Err(_) => {
                refused.push((
                    display(&paths.devices().join(&device)),
                    "not a plain folder".to_owned(),
                ));
                continue;
            }
        };
        let mut files: Vec<(u64, String)> = dir
            .names(MAX_DIR_ENTRIES)?
            .into_iter()
            .filter_map(|name| Some((parse_record_name(&name)?, name)))
            .collect();
        files.sort();
        if records.len() + files.len() > MAX_RECORDS {
            return Err("the shared folder holds more records than this version reads".into());
        }
        for (seq, name) in files {
            match read_record(&dir, &name, &device, seq, &mut bytes) {
                Ok(Some(record)) => records.push(record),
                // A newer format: left for a newer version of the application.
                Ok(None) => {}
                Err(why) => refused.push((
                    display(&paths.devices().join(&device).join(&name)),
                    why.to_owned(),
                )),
            }
            if bytes > MAX_RECORDS_BYTES {
                return Err("the shared folder holds more records than this version reads".into());
            }
        }
    }
    Ok((records, refused))
}

fn read_record(
    dir: &Dir,
    name: &str,
    device: &str,
    seq: u64,
    total: &mut u64,
) -> Result<Option<Record>, &'static str> {
    let file = dir
        .file(name)
        .map_err(|_| "not a record file")?
        .ok_or("unreadable")?;
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "unreadable")?;
    *total += bytes.len() as u64;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err("not a record file");
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| "not JSON")?;
    if value.get("schema").and_then(|s| s.as_str()) != Some(SCHEMA) {
        return Ok(None);
    }
    let record: Record = serde_json::from_value(value).map_err(|_| "malformed record")?;
    check_record(&record, device, seq)?;
    Ok(Some(record))
}

/// Whether a record is well formed and honest about where it is: its device and sequence number
/// are its folder and file name, and its content is what its kind allows.
pub fn check_record(record: &Record, device: &str, seq: u64) -> Result<(), &'static str> {
    if record.device != device || record.seq != seq || seq == 0 {
        return Err("record does not match its place");
    }
    if record.host.is_empty()
        || record.host.chars().count() > 64
        || record.host.chars().any(char::is_control)
    {
        return Err("bad host");
    }
    if record.merged.len() > MAX_MERGED
        || record
            .base
            .iter()
            .chain(&record.merged)
            .any(|id| !record_id_ok(id))
    {
        return Err("bad reference");
    }
    let (kind, key) = record.object.split_once('/').ok_or("bad object")?;
    match (&record.body, kind) {
        (Body::Capability(body), "capability") => check_capability(key, body),
        (Body::Preset(body), "preset") if store::uuid_ok(key) => {
            text_ok(&body.name, 80, true)?;
            text_ok(&body.description, 500, false)?;
            items_ok(&body.items)
        }
        (Body::Selection(body), "selection") if store::uuid_ok(key) => {
            text_ok(&body.name, 80, true)?;
            if !matches!(body.provider.as_str(), "claude_code" | "codex")
                || body.composites.len() > 16
            {
                return Err("bad selection");
            }
            items_ok(&body.items)
        }
        (Body::Deleted, "preset" | "selection") if store::uuid_ok(key) => Ok(()),
        _ => Err("object and content disagree"),
    }
}

fn check_capability(identity: &str, body: &CapabilityBody) -> Result<(), &'static str> {
    if !identity_form(identity) || !hex64(&body.tree_hash) {
        return Err("bad identity");
    }
    if body.files.is_empty() || body.files.len() > MAX_FILES {
        return Err("bad file list");
    }
    let mut seen = BTreeSet::new();
    let mut entries: Vec<(&str, &str)> = Vec::with_capacity(body.files.len());
    for file in &body.files {
        if !hex64(&file.sha256)
            || file.size > MAX_FILE_BYTES as u64
            || !seen.insert(file.path.as_str())
        {
            return Err("bad file");
        }
        if file.path.is_empty()
            || file.path.len() > 256
            || file.path.starts_with('/')
            || file
                .path
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err("bad path");
        }
        entries.push((file.path.as_str(), file.sha256.as_str()));
    }
    if tree_of_digests(&mut entries) != body.tree_hash {
        return Err("content does not match its digest");
    }
    Ok(())
}

/// `store::tree_digest`, from the files' digests instead of their bytes.
fn tree_of_digests(entries: &mut [(&str, &str)]) -> String {
    use sha2::{Digest, Sha256};
    entries.sort();
    let mut hasher = Sha256::new();
    for (path, hash) in entries.iter() {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

fn items_ok(items: &[SharedItem]) -> Result<(), &'static str> {
    if items.len() > 128 {
        return Err("too many items");
    }
    let mut seen = BTreeSet::new();
    for item in items {
        if !identity_form(&item.identity)
            || !seen.insert(item.identity.as_str())
            || item.tree_hash.as_deref().is_some_and(|hash| !hex64(hash))
        {
            return Err("bad item");
        }
    }
    Ok(())
}

fn text_ok(value: &str, max: usize, required: bool) -> Result<(), &'static str> {
    if (required && value.trim().is_empty())
        || value.chars().count() > max
        || value.chars().any(char::is_control)
    {
        return Err("bad text");
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Local items and shared items

fn shared_items(
    items: &[Item],
    ids: &BTreeMap<String, &SyncCapability>,
) -> Option<Vec<SharedItem>> {
    items
        .iter()
        .map(|item| {
            let capability = ids.get(&item.capability)?;
            let tree_hash = match item.revision {
                Some(number) => Some(
                    capability
                        .revisions
                        .iter()
                        .find(|r| r.number == number)?
                        .tree_hash
                        .clone(),
                ),
                None => None,
            };
            Some(SharedItem {
                identity: capability.identity.clone(),
                tree_hash,
            })
        })
        .collect()
}

/// The local items of shared ones, or `None` while a capability or pinned revision is not here yet.
fn local_items(
    store: &Store,
    items: &[SharedItem],
    identities: &BTreeMap<String, &SyncCapability>,
) -> Result<Option<Vec<Item>>, String> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Some(capability) = identities.get(&item.identity) else {
            return Ok(None);
        };
        let revision = match &item.tree_hash {
            Some(tree) => match store
                .tree_revision(&capability.id, tree)
                .map_err(store_text)?
            {
                Some(number) => Some(number),
                None => return Ok(None),
            },
            None => None,
        };
        out.push(Item {
            capability: capability.id.clone(),
            revision,
        });
    }
    Ok(Some(out))
}

fn body_items(body: &Body) -> &[SharedItem] {
    match body {
        Body::Preset(preset) => &preset.items,
        Body::Selection(selection) => &selection.items,
        _ => &[],
    }
}

// ---------------------------------------------------------------------------------------------
// Status

fn status(
    paths: &Paths,
    host: &str,
    local: &Local,
    records: &[Record],
    pending: Vec<Note>,
) -> Status {
    let mine = records
        .iter()
        .filter(|r| r.device == local.device)
        .map(|r| r.seq)
        .max()
        .unwrap_or(0);
    let mut peers: BTreeMap<String, Peer> = BTreeMap::new();
    for record in records.iter().filter(|r| r.device != local.device) {
        let peer = peers.entry(record.device.clone()).or_insert(Peer {
            device: record.device.clone(),
            host: record.host.clone(),
            last_heard: None,
            applied_mine: 0,
            published_mine: mine,
        });
        peer.host = record.host.clone();
        peer.last_heard = peer.last_heard.max(Some(record.at));
    }
    let devices = Dir::open(paths, &["v1", "devices"], false).ok().flatten();
    let names = devices
        .as_ref()
        .and_then(|d| d.names(MAX_DEVICE_ENTRIES).ok())
        .unwrap_or_default();
    if let Some(devices) = &devices {
        for device in names {
            if device == local.device || !device_ok(&device) {
                continue;
            }
            let Some(dir) = devices.child(&device, false).ok().flatten() else {
                continue;
            };
            let applied = read_applied(&dir);
            let Some(applied) = applied.filter(|a| a.device == device) else {
                continue;
            };
            let peer = peers.entry(device.clone()).or_insert(Peer {
                device: device.clone(),
                host: clean_host(&applied.host),
                last_heard: None,
                applied_mine: 0,
                published_mine: mine,
            });
            peer.applied_mine = applied
                .seen
                .get(&local.device)
                .copied()
                .unwrap_or(0)
                .min(mine);
            peer.last_heard = peer.last_heard.max(Some(applied.at));
        }
    }
    Status {
        enabled: true,
        folder: display(&paths.shared),
        device: local.device.clone(),
        host: host.to_owned(),
        generation: local.generation,
        last_run: Some(now()),
        last_success: local.last_success,
        error: None,
        published: mine,
        pending,
        conflicts: local.conflicts.clone(),
        rejected: local.rejected.clone(),
        removed: local.removed.clone(),
        peers: peers.into_values().collect(),
    }
}

// ---------------------------------------------------------------------------------------------
// The background worker

/// What the window holds of synchronization: its last status and a way to ask for a run.
#[derive(Clone)]
pub struct Handle {
    status: Arc<Mutex<Status>>,
    nudge: Sender<()>,
}

impl Handle {
    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
    /// Asks for a run soon, after a change in this library.
    pub fn nudge(&self) {
        let _ = self.nudge.send(());
    }
}

/// Starts synchronization for this library: a run at start, after each local change, when the
/// synchronizer brings a file into the shared folder (inotify, not polling), and every ten minutes
/// as a recovery. It never blocks the window for longer than one run holds the library.
pub fn start(store: Arc<Mutex<Store>>, paths: Paths) -> Handle {
    let host = host_name();
    let status = Arc::new(Mutex::new(read_status(&paths, &host)));
    let (nudge, wake) = mpsc::channel();
    let handle = Handle {
        status: status.clone(),
        nudge: nudge.clone(),
    };
    let watch_paths = paths.clone();
    let _ = std::thread::Builder::new()
        .name("arsenal-sync-watch".into())
        .spawn(move || watch(&watch_paths, nudge));
    let _ = std::thread::Builder::new()
        .name("arsenal-sync".into())
        .spawn(move || run(store, paths, host, status, wake));
    handle
}

fn run(
    store: Arc<Mutex<Store>>,
    paths: Paths,
    host: String,
    status: Arc<Mutex<Status>>,
    wake: Receiver<()>,
) {
    loop {
        let outcome = match store.lock() {
            Ok(mut store) => reconcile(&mut store, &paths, &host),
            Err(_) => Err("library unavailable".into()),
        };
        let more = outcome
            .as_ref()
            .is_ok_and(|s| s.pending.iter().any(|n| n.detail == MORE_NEXT_RUN));
        if let Ok(mut current) = status.lock() {
            match outcome {
                Ok(next) => *current = next,
                Err(error) if error == "busy" => {}
                Err(error) => {
                    current.error = Some(error);
                    current.last_run = Some(now());
                }
            }
        }
        let wait = if more { 5 } else { 600 };
        match wake.recv_timeout(Duration::from_secs(wait)) {
            Ok(()) => {
                // Many files arrive together: wait for them to settle, then run once.
                std::thread::sleep(Duration::from_millis(1500));
                while wake.try_recv().is_ok() {}
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn watch(paths: &Paths, nudge: Sender<()>) {
    use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify};
    let Ok(inotify) = Inotify::init(InitFlags::IN_CLOEXEC) else {
        return;
    };
    // A watch only asks for a run, which then reads the folder as `Dir` does: no link followed.
    let flags = AddWatchFlags::IN_MOVED_TO
        | AddWatchFlags::IN_CLOSE_WRITE
        | AddWatchFlags::IN_CREATE
        | AddWatchFlags::IN_DONT_FOLLOW
        | AddWatchFlags::IN_ONLYDIR;
    let _ = Dir::open(paths, &["v1", "devices"], true);
    let _ = Dir::open(paths, &["v1", "blobs"], true);
    let mut watched: BTreeSet<PathBuf> = BTreeSet::new();
    let add = |inotify: &Inotify, watched: &mut BTreeSet<PathBuf>| {
        let mut dirs = vec![paths.devices(), paths.blobs()];
        if let Ok(entries) = fs::read_dir(paths.devices()) {
            dirs.extend(
                entries
                    .flatten()
                    .take(MAX_DEVICE_ENTRIES)
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|e| e.path()),
            );
        }
        for dir in dirs {
            if !watched.contains(&dir) && inotify.add_watch(&dir, flags).is_ok() {
                watched.insert(dir);
            }
        }
    };
    add(&inotify, &mut watched);
    loop {
        match inotify.read_events() {
            Ok(events) => {
                if events
                    .iter()
                    .any(|e| e.mask.contains(AddWatchFlags::IN_CREATE))
                {
                    add(&inotify, &mut watched);
                }
                // Our own staging files never land here; renames into place do, and are harmless.
                if nudge.send(()).is_err() {
                    return;
                }
            }
            Err(nix::errno::Errno::EINTR) => {}
            Err(_) => return,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Small pieces

fn load_local(paths: &Paths) -> Result<Local, String> {
    if let Some(local) = load_local_existing(paths) {
        return Ok(local);
    }
    let local = Local {
        device: uuid::Uuid::new_v4().simple().to_string(),
        next_seq: 1,
        ..Local::default()
    };
    save_local(paths, &local)?;
    Ok(local)
}

fn load_local_existing(paths: &Paths) -> Option<Local> {
    let bytes = fs::read(paths.local.join("state.json")).ok()?;
    let local: Local = serde_json::from_slice(&bytes).ok()?;
    device_ok(&local.device).then_some(local)
}

fn save_local(paths: &Paths, local: &Local) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(local).map_err(|e| e.to_string())?;
    let temp = paths.local.join(".state.json.tmp");
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| format!("state: {e}"))?;
        file.write_all(&bytes).map_err(|e| format!("state: {e}"))?;
        file.sync_all().map_err(|e| format!("state: {e}"))?;
    }
    fs::rename(&temp, paths.local.join("state.json")).map_err(|e| format!("state: {e}"))
}

fn note_rejected(local: &mut Local, files: &[(String, String)]) {
    for (path, why) in files {
        let key = format!("file:{path}");
        if local.refused.insert(key, why.clone()).is_none() {
            push_note(&mut local.rejected, note("record", path, why));
        }
    }
}

fn push_note(notes: &mut Vec<Note>, note: Note) {
    if notes
        .iter()
        .any(|n| n.kind == note.kind && n.name == note.name && n.detail == note.detail)
    {
        return;
    }
    notes.push(note);
    if notes.len() > MAX_NOTES {
        let extra = notes.len() - MAX_NOTES;
        notes.drain(..extra);
    }
}

fn note(kind: &str, name: &str, detail: &str) -> Note {
    Note {
        at: now(),
        kind: kind.to_owned(),
        name: name.to_owned(),
        detail: detail.to_owned(),
    }
}

fn object_name(record: &Record) -> String {
    match &record.body {
        Body::Deleted => record.object.clone(),
        body => body_name(body),
    }
}

fn body_name(body: &Body) -> String {
    match body {
        Body::Capability(c) => c.name.clone(),
        Body::Preset(p) => p.name.clone(),
        Body::Selection(s) => s.name.clone(),
        Body::Deleted => String::new(),
    }
}

fn kind_of(key: &str) -> &str {
    key.split('/').next().unwrap_or("object")
}

fn conflict_name(name: &str, host: &str) -> String {
    let suffix = format!(" (conflict, {})", host.chars().take(20).collect::<String>());
    let keep = 80 - suffix.chars().count();
    format!("{}{suffix}", name.chars().take(keep).collect::<String>())
}

/// A UUID-shaped id derived from text, so every computer names a conflict copy the same way.
fn derived_uuid(text: &str) -> String {
    let hash = store::digest(text.as_bytes());
    format!(
        "{}-{}-{}-{}-{}",
        &hash[0..8],
        &hash[8..12],
        &hash[12..16],
        &hash[16..20],
        &hash[20..32]
    )
}

fn current_tree(capability: &SyncCapability) -> String {
    revision_of(capability, capability.current)
        .tree_hash
        .clone()
}

fn revision_of(capability: &SyncCapability, number: i64) -> &store::SyncRevision {
    capability
        .revisions
        .iter()
        .find(|r| r.number == number)
        .or_else(|| capability.revisions.last())
        .expect("a capability has a revision")
}

fn record_name(seq: u64) -> String {
    format!("r-{seq:010}.json")
}

fn parse_record_name(name: &str) -> Option<u64> {
    let digits = name.strip_prefix("r-")?.strip_suffix(".json")?;
    (digits.len() == 10 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())?
}

fn last_seq(dir: &Dir) -> Result<u64, String> {
    Ok(dir
        .names(MAX_DIR_ENTRIES)?
        .iter()
        .filter_map(|name| parse_record_name(name))
        .max()
        .unwrap_or(0))
}

fn device_ok(name: &str) -> bool {
    name.len() == 32
        && name
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn record_id_ok(id: &str) -> bool {
    id.split_once(':').is_some_and(|(device, seq)| {
        device_ok(device)
            && !seq.is_empty()
            && seq.len() <= 10
            && seq.bytes().all(|b| b.is_ascii_digit())
    })
}

fn hex64(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn identity_form(identity: &str) -> bool {
    let (hash, suffix) = identity
        .split_once('#')
        .map_or((identity, None), |(h, n)| (h, Some(n)));
    hex64(hash)
        && suffix
            .is_none_or(|n| !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

fn clean_host(host: &str) -> String {
    let host: String = host.chars().filter(|c| !c.is_control()).take(64).collect();
    if host.trim().is_empty() {
        "this computer".into()
    } else {
        host
    }
}

fn host_name() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|name| name.trim().to_owned())
        .unwrap_or_default()
}

/// A folder of the shared folder, opened without following a symbolic link at any level of it:
/// a link another computer plants there cannot redirect a read or a write. The folder that holds
/// the shared one (`~/.local/share`) is this computer's own and is resolved as usual.
struct Dir {
    file: File,
}

impl Dir {
    fn open(paths: &Paths, parts: &[&str], create: bool) -> Result<Option<Dir>, String> {
        let base = paths.shared.parent().ok_or("folder: bad path")?;
        let anchor = base.parent().ok_or("folder: bad path")?;
        let mut names = Vec::new();
        for part in [base, paths.shared.as_path()] {
            names.push(
                part.file_name()
                    .and_then(|n| n.to_str())
                    .ok_or("folder: bad path")?
                    .to_owned(),
            );
        }
        names.extend(parts.iter().map(|part| (*part).to_owned()));
        if create {
            make_dir(anchor).map_err(|e| format!("folder: {e}"))?;
        }
        let file = match open_flags(anchor, true, false) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => {
                return Ok(None);
            }
            Err(error) => return Err(format!("folder: {error}")),
        };
        let mut dir = Dir { file };
        for name in names {
            match dir.child(&name, create)? {
                Some(next) => dir = next,
                None => return Ok(None),
            }
        }
        Ok(Some(dir))
    }

    /// A name in this folder as the system can open it, through the folder's descriptor.
    fn at(&self, name: &str) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd())).join(name)
    }

    fn child(&self, name: &str, create: bool) -> Result<Option<Dir>, String> {
        if !plain_name(name) {
            return Err("folder: bad name".into());
        }
        if create {
            match fs::DirBuilder::new().mode(0o700).create(self.at(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(format!("folder: {error}")),
            }
        }
        match open_flags(&self.at(name), true, true) {
            Ok(file) => Ok(Some(Dir { file })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !create => Ok(None),
            Err(_) => Err(format!("folder: {name} is not a plain folder")),
        }
    }

    /// A plain file of this folder: never a link, a pipe or a device.
    fn file(&self, name: &str) -> Result<Option<File>, String> {
        if !plain_name(name) {
            return Err("bad name".into());
        }
        let file = match open_flags(&self.at(name), false, true) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("not a plain file".into()),
        };
        match file.metadata() {
            Ok(meta) if meta.is_file() => Ok(Some(file)),
            _ => Err("not a plain file".into()),
        }
    }

    /// The names in this folder, at most `limit` of them; more is an error, counted while reading.
    fn names(&self, limit: usize) -> Result<Vec<String>, String> {
        let entries = fs::read_dir(self.at(".")).map_err(|e| format!("folder: {e}"))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| format!("folder: {e}"))?;
            if names.len() >= limit {
                return Err("the shared folder holds more files than this version reads".into());
            }
            if let Ok(name) = entry.file_name().into_string() {
                names.push(name);
            }
        }
        Ok(names)
    }
}

fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\0')
}

fn open_flags(path: &Path, directory: bool, no_follow: bool) -> std::io::Result<File> {
    use nix::libc;
    let mut flags = libc::O_CLOEXEC | libc::O_NONBLOCK;
    if directory {
        flags |= libc::O_DIRECTORY;
    }
    if no_follow {
        flags |= libc::O_NOFOLLOW;
    }
    OpenOptions::new().read(true).custom_flags(flags).open(path)
}

/// Keeps what another computer's deletion removes from this library, in this computer's own
/// folder, and says where.
fn keep_deleted(paths: &Paths, key: &str, body: &Body) -> Result<String, String> {
    let dir = paths.local.join("trash");
    make_dir(&dir).map_err(|e| format!("trash: {e}"))?;
    let name = format!(
        "{}-{}.json",
        key.replace('/', "-"),
        uuid::Uuid::new_v4().simple()
    );
    let bytes = serde_json::to_vec_pretty(body).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("trash: {e}"))?;
    file.write_all(&bytes).map_err(|e| format!("trash: {e}"))?;
    file.sync_all().map_err(|e| format!("trash: {e}"))?;
    Ok(display(&path))
}

fn make_dir(dir: &Path) -> std::io::Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

fn display(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && text.starts_with(&home) => {
            format!("~{}", &text[home.len()..])
        }
        _ => text,
    }
}

fn store_text(error: StoreError) -> String {
    format!("library: {error:?}").to_lowercase()
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
