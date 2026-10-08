//! Window material for the main window.
//!
//! The mode is decided before the window exists, because transparency is a
//! creation-time attribute. Every failure degrades toward the opaque window;
//! nothing in this module may stop startup.

/// What the main window looks like. The frontend reads it from
/// `document.documentElement.dataset.glass` to pick its panel veil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassMode {
    /// Transparent window plus a KWin/X11 blur request behind it.
    Native,
    /// Transparent window without a blur request (compositor, no KWin blur).
    Transparent,
    /// Opaque window; the frontend simulates the glass inside the page.
    Opaque,
}

impl GlassMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Transparent => "transparent",
            Self::Opaque => "opaque",
        }
    }

    pub fn transparent(self) -> bool {
        self != Self::Opaque
    }

    pub fn kwin_blur(self) -> bool {
        self == Self::Native
    }

    /// Runs before the page scripts. `documentElement` may not exist yet at
    /// document start, so the attribute is set as soon as it does.
    pub fn bootstrap_script(self) -> String {
        format!(
            "(function () {{\
               var set = function () {{ document.documentElement.dataset.glass = '{}'; }};\
               if (document.documentElement) set();\
               else document.addEventListener('readystatechange', set);\
             }})();",
            self.as_str()
        )
    }
}

/// Facts about the running session that the mode depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionFacts {
    /// The GDK display is the X11 backend.
    pub x11: bool,
    /// The screen has a compositing manager. Without one, a transparent
    /// window shows black, so transparency is not requested.
    pub composited: bool,
    /// The desktop session is KDE Plasma (KWin is the window manager).
    pub kde: bool,
}

/// Pure decision: testable without a display.
pub fn plan(facts: SessionFacts) -> GlassMode {
    match (facts.composited, facts.x11 && facts.kde) {
        (false, _) => GlassMode::Opaque,
        (true, true) => GlassMode::Native,
        (true, false) => GlassMode::Transparent,
    }
}

/// KDE is announced by `XDG_CURRENT_DESKTOP` (a colon-separated list) or by
/// `KDE_SESSION_VERSION`.
pub fn kde_session(xdg_current_desktop: Option<&str>, kde_session_version: Option<&str>) -> bool {
    kde_session_version.is_some_and(|version| !version.trim().is_empty())
        || xdg_current_desktop
            .is_some_and(|desktops| desktops.split(':').any(|d| d.eq_ignore_ascii_case("KDE")))
}

#[cfg(target_os = "linux")]
pub fn probe() -> SessionFacts {
    use gtk::glib::ObjectExt;
    use std::env;

    let x11 = gtk::gdk::Display::default()
        .is_some_and(|display| display.type_().name() == "GdkX11Display");
    let composited = gtk::gdk::Screen::default().is_some_and(|screen| screen.is_composited());
    let kde = kde_session(
        env::var("XDG_CURRENT_DESKTOP").ok().as_deref(),
        env::var("KDE_SESSION_VERSION").ok().as_deref(),
    );
    SessionFacts {
        x11,
        composited,
        kde,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn probe() -> SessionFacts {
    SessionFacts {
        x11: false,
        composited: false,
        kde: false,
    }
}

/// Requests KWin's blur behind the window. An empty region means the whole
/// window, so the property is written once per GDK window and never on
/// resize. Other window managers ignore the property.
#[cfg(target_os = "linux")]
pub fn apply(window: &tauri::WebviewWindow, mode: GlassMode) {
    use gtk::prelude::*;

    if !mode.kwin_blur() {
        return;
    }
    let Ok(gtk_window) = window.gtk_window() else {
        return;
    };
    if gtk_window.is_realized() {
        request_kwin_blur(&gtk_window);
    } else {
        gtk_window.connect_realize(request_kwin_blur);
    }
}

#[cfg(not(target_os = "linux"))]
pub fn apply(_window: &tauri::WebviewWindow, _mode: GlassMode) {}

#[cfg(target_os = "linux")]
fn request_kwin_blur(window: &gtk::ApplicationWindow) {
    use gtk::prelude::*;

    let Some(gdk_window) = window.window() else {
        return;
    };
    gtk::gdk::property_change(
        &gdk_window,
        &gtk::gdk::Atom::intern("_KDE_NET_WM_BLUR_BEHIND_REGION"),
        &gtk::gdk::Atom::intern("CARDINAL"),
        32,
        gtk::gdk::PropMode::Replace,
        gtk::gdk::ChangeData::ULongs(&[]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(x11: bool, composited: bool, kde: bool) -> SessionFacts {
        SessionFacts {
            x11,
            composited,
            kde,
        }
    }

    #[test]
    fn kwin_x11_with_compositor_is_native() {
        assert_eq!(plan(facts(true, true, true)), GlassMode::Native);
    }

    #[test]
    fn compositor_without_kwin_blur_is_transparent() {
        assert_eq!(plan(facts(true, true, false)), GlassMode::Transparent);
        assert_eq!(plan(facts(false, true, true)), GlassMode::Transparent);
        assert_eq!(plan(facts(false, true, false)), GlassMode::Transparent);
    }

    #[test]
    fn no_compositor_is_always_opaque() {
        assert_eq!(plan(facts(true, false, true)), GlassMode::Opaque);
        assert_eq!(plan(facts(false, false, false)), GlassMode::Opaque);
    }

    #[test]
    fn only_native_requests_blur_and_only_opaque_skips_transparency() {
        assert!(GlassMode::Native.kwin_blur());
        assert!(!GlassMode::Transparent.kwin_blur());
        assert!(!GlassMode::Opaque.kwin_blur());
        assert!(GlassMode::Native.transparent());
        assert!(GlassMode::Transparent.transparent());
        assert!(!GlassMode::Opaque.transparent());
    }

    #[test]
    fn kde_is_detected_from_desktop_list_or_version() {
        assert!(kde_session(Some("KDE"), None));
        assert!(kde_session(Some("ubuntu:KDE"), None));
        assert!(kde_session(Some("kde"), None));
        assert!(kde_session(None, Some("6")));
        assert!(!kde_session(None, Some("  ")));
        assert!(!kde_session(Some("GNOME"), None));
        assert!(!kde_session(Some("XFCE:GNOME-Classic"), None));
        assert!(!kde_session(None, None));
    }

    #[test]
    fn bootstrap_script_sets_the_mode_attribute() {
        let script = GlassMode::Native.bootstrap_script();
        assert!(script.contains("dataset.glass = 'native'"));
        assert!(GlassMode::Opaque.bootstrap_script().contains("'opaque'"));
    }
}
