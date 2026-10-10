//! What a preference can be. Every field is a closed word, a bounded number or a flag:
//! there is no string, no path and no map in it, so there is nowhere to put a command,
//! a secret or a policy. A preference changes how something is shown, never what is
//! allowed.
use serde::{Deserialize, Serialize};

/// The version of the file a preference set is stored in. A file with a larger one was
/// written by a newer Wyss Console and is not interpreted.
pub const SCHEMA_VERSION: u32 = 1;

pub const FONT_SIZE: std::ops::RangeInclusive<u8> = 10..=24;
pub const SCROLLBACK: std::ops::RangeInclusive<u32> = 500..=50_000;

/// The view the window opens on. Settings is not one of them: it is reached on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LandingPage {
    Console,
    Sessions,
    Agents,
    Approvals,
    Events,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorStyle {
    Block,
    Bar,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeFormat {
    /// The language and region of the session decide.
    System,
    H12,
    H24,
}

/// The filter the Events view starts with. It narrows what is shown of what was read;
/// it never changes what the authority records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventFilter {
    All,
    Sessions,
    Runtime,
    Turns,
    Approvals,
    Security,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub landing_page: LandingPage,
}

/// How the human terminal looks. What it runs, where and with what is not here: human
/// terminals are the desktop's own and take no preference for any of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
    pub font_size: u8,
    pub scrollback: u32,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Events {
    pub time_format: TimeFormat,
    pub default_filter: EventFilter,
}

/// The preferences of one person on one desktop. Not the authority's, and not a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub schema_version: u32,
    pub interface: Interface,
    pub terminal: Terminal,
    pub events: Events,
}

/// A preference set that is well formed and out of bounds, or of another version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invalid;

impl Default for Preferences {
    /// What the product looked like before it had any: the console first, the terminal
    /// as it was, every event shown.
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            interface: Interface {
                landing_page: LandingPage::Console,
            },
            terminal: Terminal {
                font_size: 16,
                scrollback: 5000,
                cursor_style: CursorStyle::Block,
                cursor_blink: false,
            },
            events: Events {
                time_format: TimeFormat::System,
                default_filter: EventFilter::All,
            },
        }
    }
}

impl Preferences {
    /// Rust is the judge of every value, whatever the window checked first.
    pub fn validate(&self) -> Result<(), Invalid> {
        if self.schema_version == SCHEMA_VERSION
            && FONT_SIZE.contains(&self.terminal.font_size)
            && SCROLLBACK.contains(&self.terminal.scrollback)
        {
            Ok(())
        } else {
            Err(Invalid)
        }
    }
}
