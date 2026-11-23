//! Interfaz GTK4 para Wyss Console.

mod activity;
mod arsenal;
mod cli;
mod credentials;
mod console;
mod distributors;
mod ghostty;
mod host;
mod i18n;
mod i18n_table;
mod icons;
mod keymap;
mod keys;
mod launches;
mod layout;
mod native;
mod palette;
mod personalization;
mod prefs;
mod procinfo;
mod project_instructions;
mod project_panel;
mod pty;
mod search;
mod selection;
mod settings;
mod term;
mod theme;
mod view;
mod wheel;
mod workspace;

use wyss_console::settings::SettingsService;

use gtk::prelude::*;
use gtk::{gio, glib};
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> glib::ExitCode {
    let mut args = std::env::args();
    args.next();
    if args.next().as_deref() == Some("panel") {
        use std::io::{stdin, stdout};
        let result = std::env::current_dir().and_then(|cwd| {
            let args: Vec<_> = args.collect();
            wyss_console::project::command(&cwd, &args, &mut stdin().lock(), &mut stdout().lock())
        });
        return match result {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("wyss-panel: {error}");
                glib::ExitCode::FAILURE
            }
        };
    }
    let service = SettingsService::from_environment();
    let snapshot = settings::Snapshot::load(&service);
    // One window: a later `wyss-console …` (a shortcut, Dolphin, a menu entry that needs a
    // terminal) reaches the running one through its command line and opens a tab there. Without a
    // session bus GApplication runs on its own, as before.
    let app = gtk::Application::builder()
        .application_id("dev.wyss.Console")
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let host: Rc<RefCell<Option<Option<host::Host>>>> = Rc::default();
    app.connect_command_line(move |app, line| {
        let args: Vec<String> = line.arguments().iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
        let mut request = match cli::parse(&args) {
            Ok(r) => r,
            Err(error) => {
                eprintln!("wyss-console: {error}");
                return 2.into();
            }
        };
        let cwd = line.cwd().map(|p| p.to_string_lossy().into_owned());
        if let (Some(dir), Some(base)) = (&request.workdir, &cwd)
            && !dir.starts_with('/')
        {
            request.workdir = Some(format!("{base}/{dir}"));
        }
        if console::handle(&request, cwd.clone()) {
            return 0.into();
        }
        // First window of this process: the host is reached only now, in the primary instance.
        if host.borrow().is_none() {
            match host::connect_or_start() {
                Ok(h) => *host.borrow_mut() = Some(h),
                Err(error) => {
                    eprintln!("wyss-console: {error}");
                    return 1.into();
                }
            }
        }
        let h = host.borrow().clone().flatten();
        build(app, console::Config { host: h, request, cwd }, service.clone(), snapshot.clone());
        0.into()
    });
    app.run()
}

fn build(app: &gtk::Application, cfg: console::Config, service: SettingsService, snapshot: settings::Snapshot) {
    let win = console::build(app, cfg, service, snapshot);

    if let Some(secs) = std::env::var("WYSS_CONSOLE_EXIT_AFTER").ok().and_then(|v| v.parse::<u32>().ok()) {
        let win = win.clone();
        glib::timeout_add_seconds_local_once(secs, move || win.close());
    }

    // WYSS_CONSOLE_SHOT=path.png renders the window through GSK into a PNG after 2 s (visual check).
    if let Ok(path) = std::env::var("WYSS_CONSOLE_SHOT") {
        let win = win.clone();
        let after = std::env::var("WYSS_CONSOLE_SHOT_AFTER").ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(2);
        glib::timeout_add_seconds_local_once(after, move || {
            let paint = gtk::WidgetPaintable::new(Some(&win));
            let (w, h) = (win.width() as f32, win.height() as f32);
            let snap = gtk::Snapshot::new();
            paint.snapshot(&snap, w as f64, h as f64);
            if let (Some(node), Some(renderer)) = (snap.to_node(), win.native().and_then(|n| n.renderer())) {
                let tex = renderer.render_texture(&node, None);
                let _ = tex.save_to_png(&path);
                eprintln!("shot: {path} renderer={}", renderer.type_().name());
            }
        });
    }
}


#[cfg(test)]
fn personalization_scratch() -> std::path::PathBuf {
    std::env::var_os("WYSS_TEST_SCRATCH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
fn personalization_round_trip(name: &str, child_test: &str) {
    let path = personalization_scratch().join(format!("native-{name}-{}", uuid::Uuid::new_v4()));
    let fixture = PersonalizationFixture(path);
    std::fs::create_dir_all(fixture.0.join("home")).unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", child_test, "--nocapture"])
        .env("WYSS_PERSONALIZATION_CHILD", &fixture.0)
        .env("HOME", fixture.0.join("home"))
        .env("XDG_CONFIG_HOME", fixture.0.join("config"))
        .env("XDG_DATA_HOME", fixture.0.join("data"))
        .env("XDG_STATE_HOME", fixture.0.join("state"))
        .env("XDG_CACHE_HOME", fixture.0.join("cache"))
        .env("XDG_RUNTIME_DIR", fixture.0.join("runtime"))
        .env("CODEX_HOME", fixture.0.join("codex"))
        .env_remove("WYSS_CONSOLE_HOST")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("WYSS_ARSENAL_SYNC")
        .output().unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(result.status.success() && stdout.contains("1 passed"), "{stdout}\n{}", String::from_utf8_lossy(&result.stderr));
}

#[cfg(test)]
fn personalization_child_root(name: &str) -> Option<std::path::PathBuf> {
    let root = std::path::PathBuf::from(std::env::var_os("WYSS_PERSONALIZATION_CHILD")?);
    assert_eq!(root.parent(), Some(personalization_scratch().as_path()));
    assert!(root.file_name().and_then(|name| name.to_str())
        .and_then(|file| file.strip_prefix(&format!("native-{name}-")))
        .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok()));
    Some(root)
}

#[cfg(test)]
struct PersonalizationFixture(std::path::PathBuf);

#[cfg(test)]
impl Drop for PersonalizationFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
