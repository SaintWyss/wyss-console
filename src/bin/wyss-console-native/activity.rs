//! The Claude / Codex readings in the status bar. Only the host reads usage sources.
use crate::{
    host::Host,
    i18n::{tr, trf},
};
use gtk::prelude::*;
use serde_json::json;
use wyss_console::native::{
    catalogue::AgentId,
    service::{ActivitySession, ActivityView},
    store,
    usage::{self, Level, Plan},
};

pub struct Strip {
    pub root: gtk::Box,
    claude: Column,
    codex: Column,
}

struct Column {
    context: Meter,
    short: Meter,
    long: Meter,
}

/// One reading: its name and the percentage, which turns amber or red as it fills.
struct Meter {
    cell: gtk::Box,
    value: gtk::Label,
    /// Shown even without a reading (as "—"), so its place in the bar is always the same.
    always: bool,
}

impl Strip {
    pub fn new() -> Self {
        let root = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        root.add_css_class("usage-strip");
        let columns = gtk::Box::new(gtk::Orientation::Horizontal, 18);
        let claude = Column::new(&columns, "Claude Code");
        let codex = Column::new(&columns, "Codex");
        root.append(&columns);
        let result = Self {
            root,
            claude,
            codex,
        };
        result.paint(&ActivityView::default(), None);
        result
    }

    pub fn paint(&self, activity: &ActivityView, focused: Option<&str>) {
        let for_provider = |provider| {
            activity
                .sessions
                .iter()
                .find(|s| s.agent == provider && Some(s.id.as_str()) == focused)
                .or_else(|| {
                    activity
                        .sessions
                        .iter()
                        .rev()
                        .filter(|s| s.agent == provider)
                        .reduce(|latest, s| {
                            if s.started_at > latest.started_at {
                                s
                            } else {
                                latest
                            }
                        })
                })
        };
        let now = store::now();
        self.claude.paint(
            activity.plans.claude,
            for_provider(AgentId::ClaudeCode),
            now,
        );
        self.codex
            .paint(activity.plans.codex, for_provider(AgentId::Codex), now);
    }
}

impl Column {
    fn new(root: &gtk::Box, title: &str) -> Self {
        let col = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        col.add_css_class("usage-column");
        let heading = gtk::Label::new(Some(title));
        heading.add_css_class("usage-provider");
        col.append(&heading);
        let context = Meter::new(&col, &tr("Context"), true);
        let short = Meter::new(&col, "5 h", false);
        let long = Meter::new(&col, &tr("Week"), false);
        root.append(&col);
        Self {
            context,
            short,
            long,
        }
    }

    fn paint(&self, plan: Plan, session: Option<&ActivitySession>, now: u64) {
        self.context
            .set(session.and_then(|s| s.context), None, false);
        // ponytail: only the 5 h window shows its reset; the weekly one is too far away to matter.
        // A stale reading shows its age instead: its reset may have passed already.
        let note = |w: Option<usage::Window>, reset: bool| {
            let w = w?;
            if w.stale(now) {
                return Some(trf("{age} ago", &[("age", &age(now.saturating_sub(w.at)))]));
            }
            let mins = w.resets_at.filter(|_| reset)?.saturating_sub(now).div_ceil(60);
            Some(format!("{}h {}m", mins / 60, mins % 60))
        };
        for (meter, w, reset) in [(&self.short, plan.five_hours, true), (&self.long, plan.weekly, false)] {
            meter.set(w.map(|r| r.used), note(w, reset), w.is_some_and(|r| r.stale(now)));
        }
    }
}

/// Age of a reading in its largest whole unit: 7m, 3h, 2d.
fn age(secs: u64) -> String {
    match secs / 60 {
        m if m < 60 => format!("{m}m"),
        m if m < 48 * 60 => format!("{}h", m / 60),
        m => format!("{}d", m / (24 * 60)),
    }
}

impl Meter {
    fn new(row: &gtk::Box, name: &str, always: bool) -> Self {
        let cell = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let label = gtk::Label::new(Some(name));
        label.add_css_class("usage-name");
        let value = gtk::Label::new(None);
        value.add_css_class("usage-value");
        cell.append(&label);
        cell.append(&value);
        row.append(&cell);
        Self { cell, value, always }
    }

    /// `hint` is the time to the next reset, or the age of a stale reading; it goes in the tooltip.
    fn set(&self, used: Option<f64>, hint: Option<String>, stale: bool) {
        for class in ["usage-warning", "usage-danger", "usage-stale"] {
            self.value.remove_css_class(class);
        }
        self.cell.set_visible(used.is_some() || self.always);
        self.value.set_tooltip_text(None);
        let Some(used) = used else {
            self.value.set_text("—");
            return;
        };
        match usage::context_level(used) {
            Level::Normal => {}
            Level::Warning => self.value.add_css_class("usage-warning"),
            Level::Danger => self.value.add_css_class("usage-danger"),
        }
        self.value.set_text(&percent(used));
        if stale {
            self.value.add_css_class("usage-stale");
        }
        let tip = match (stale, hint) {
            (true, Some(age)) => Some(format!("{} · {age}", tr("Stale reading"))),
            (true, None) => Some(tr("Stale reading")),
            (false, Some(time)) => Some(trf("Resets in {time}", &[("time", &time)])),
            (false, None) => None,
        };
        self.value.set_tooltip_text(tip.as_deref());
    }
}

fn percent(value: f64) -> String {
    // Whole number, floored, so 74.9 never reads as the 75 that turns the bar red.
    format!("{}%", value.floor())
}

pub fn snapshot(host: Option<&Host>) -> (ActivityView, bool) {
    if let Some(activity) = host
        .and_then(|host| host.call("native_activity", json!({})).ok())
        .and_then(|v| serde_json::from_value(v).ok())
    {
        return (activity, true);
    }
    let plans = wyss_console::native::state_paths()
        .map_or_else(usage::Plans::default, |(base, _)| {
            usage::cached_plans(&base.join("usage.json"))
        });
    (
        ActivityView {
            plans,
            sessions: Vec::new(),
        },
        false,
    )
}

#[cfg(test)]
mod age_tests {
    #[test]
    fn age_uses_the_largest_whole_unit() {
        assert_eq!(super::age(59), "0m");
        assert_eq!(super::age(15 * 60), "15m");
        assert_eq!(super::age(3 * 3600 + 59), "3h");
        assert_eq!(super::age(47 * 3600), "47h");
        assert_eq!(super::age(3 * 86400), "3d");
    }
}
