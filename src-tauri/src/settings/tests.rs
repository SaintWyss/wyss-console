//! The preference store and service against private temporary directories. Nothing here
//! touches the account running the tests: every path is under a directory of its own.
use super::*;
use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink},
    path::PathBuf,
    sync::{Arc, Barrier},
    thread,
};

/// A clean account: a private configuration root, and a state directory standing for the
/// authority's, which no preference operation may ever touch.
struct Account {
    root: PathBuf,
}
impl Account {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-st-{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for dir in ["config", "state/wyss-console"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .recursive(true)
                .create(root.join(dir))
                .unwrap();
        }
        fs::write(
            root.join("state/wyss-console/control.sqlite3"),
            b"canonical",
        )
        .unwrap();
        Self { root }
    }
    fn store(&self) -> Store {
        Store::at(self.root.join("config")).unwrap()
    }
    fn dir(&self) -> PathBuf {
        self.root.join("config/wyss-console")
    }
    fn file(&self) -> PathBuf {
        self.dir().join("preferences.json")
    }
    fn rejected(&self) -> PathBuf {
        self.dir().join("preferences.invalid.json")
    }
    fn service(&self) -> SettingsService {
        SettingsService::open(Some(self.store()))
    }
    /// Writes `bytes` as the saved file, the way a previous run would have: private.
    fn plant(&self, bytes: &[u8]) {
        fs::DirBuilder::new()
            .mode(0o700)
            .recursive(true)
            .create(self.dir())
            .unwrap();
        fs::write(self.file(), bytes).unwrap();
        fs::set_permissions(self.file(), fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn names(&self) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(self.dir())
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
    fn canonical_untouched(&self) {
        assert_eq!(
            fs::read(self.root.join("state/wyss-console/control.sqlite3")).unwrap(),
            b"canonical"
        );
    }
}
impl Drop for Account {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn changed() -> Preferences {
    let mut next = Preferences::default();
    next.interface.landing_page = LandingPage::Events;
    next.terminal.font_size = 20;
    next.terminal.scrollback = 12_000;
    next.terminal.cursor_style = CursorStyle::Bar;
    next.terminal.cursor_blink = true;
    next.events.time_format = TimeFormat::H24;
    next.events.default_filter = EventFilter::Approvals;
    next
}
fn json(preferences: &Preferences) -> Vec<u8> {
    serde_json::to_vec_pretty(preferences).unwrap()
}

// ---------------------------------------------------------------- defaults and shape

#[test]
fn defaults_are_what_the_product_looked_like_before_it_had_any() {
    let defaults = Preferences::default();
    assert_eq!(defaults.schema_version, 3);
    assert_eq!(defaults.interface.landing_page, LandingPage::Console);
    assert_eq!(defaults.interface.theme, AppearanceTheme::Dark);
    assert_eq!(defaults.interface.language, Language::En);
    assert_eq!(defaults.terminal.font_size, 16);
    assert_eq!(defaults.terminal.scrollback, 5000);
    assert_eq!(defaults.terminal.cursor_style, CursorStyle::Block);
    assert!(!defaults.terminal.cursor_blink);
    assert_eq!(defaults.events.time_format, TimeFormat::System);
    assert_eq!(defaults.events.default_filter, EventFilter::All);
    assert!(defaults.validate().is_ok());
}

#[test]
fn a_clean_account_reads_defaults_and_creates_nothing() {
    let account = Account::new();
    let service = account.service();
    let view = service.read();
    assert_eq!(view.storage, Storage::Ready);
    assert_eq!(view.preferences, Preferences::default());
    assert!(!account.dir().exists(), "reading must not create anything");
}

#[test]
fn the_vocabulary_has_no_field_for_anything_that_decides() {
    // Every key a preference set can serialize, spelled out. A new key is a decision
    // somebody has to make on purpose, here, with this list in front of them.
    fn keys(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
        if let Some(map) = value.as_object() {
            for (key, inner) in map {
                let name = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                out.push(name.clone());
                keys(inner, &name, out);
            }
        }
    }
    let mut found = Vec::new();
    keys(
        &serde_json::to_value(Preferences::default()).unwrap(),
        "",
        &mut found,
    );
    found.sort();
    let mut expected = [
        "events",
        "events.default_filter",
        "events.time_format",
        "interface",
        "interface.landing_page",
        "interface.language",
        "interface.theme",
        "schema_version",
        "terminal",
        "terminal.cursor_blink",
        "terminal.cursor_style",
        "terminal.font_size",
        "terminal.scrollback",
    ]
    .map(String::from)
    .to_vec();
    expected.sort();
    assert_eq!(found, expected);
    // And no value of it is free text: nothing a command or a secret could be put in.
    fn no_strings_but_words(value: &serde_json::Value) {
        match value {
            serde_json::Value::String(word) => {
                assert!(
                    word.len() <= 16
                        && word
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                );
            }
            serde_json::Value::Object(map) => map.values().for_each(no_strings_but_words),
            _ => {}
        }
    }
    no_strings_but_words(&serde_json::to_value(changed()).unwrap());
}

// ---------------------------------------------------------------- persistence

#[test]
fn preferences_persist_across_a_restart_of_the_service() {
    let account = Account::new();
    let first = account.service();
    assert_eq!(first.update(changed()).unwrap().preferences, changed());
    drop(first);
    let second = account.service();
    let view = second.read();
    assert_eq!(view.storage, Storage::Ready);
    assert_eq!(view.preferences, changed());
    account.canonical_untouched();
}

#[test]
fn schema_one_preferences_gain_the_dark_default_without_a_read_write_and_upgrade_on_next_change() {
    let account = Account::new();
    let old = br#"{"schema_version":1,"interface":{"landing_page":"sessions"},"terminal":{"font_size":20,"scrollback":12000,"cursor_style":"bar","cursor_blink":true},"events":{"time_format":"h24","default_filter":"approvals"}}"#;
    account.plant(old);
    let service = account.service();
    let view = service.read();
    assert_eq!(view.storage, Storage::Ready);
    assert_eq!(view.preferences.schema_version, 3);
    assert_eq!(
        view.preferences.interface.landing_page,
        LandingPage::Sessions
    );
    assert_eq!(view.preferences.interface.theme, AppearanceTheme::Dark);
    assert_eq!(view.preferences.interface.language, Language::En);
    assert_eq!(view.preferences.terminal.font_size, 20);
    assert_eq!(
        view.preferences.events.default_filter,
        EventFilter::Approvals
    );
    assert_eq!(fs::read(account.file()).unwrap(), old);

    let mut upgraded = view.preferences;
    upgraded.interface.theme = AppearanceTheme::HighContrast;
    service.update(upgraded).unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(account.file()).unwrap()).unwrap();
    assert_eq!(saved["schema_version"], 3);
    assert_eq!(saved["interface"]["theme"], "high_contrast");
    assert_eq!(account.service().read().preferences, upgraded);
}

#[test]
fn schema_two_preferences_gain_english_without_a_read_write_and_upgrade_on_next_change() {
    let account = Account::new();
    let old = br#"{"schema_version":2,"interface":{"landing_page":"events","theme":"light"},"terminal":{"font_size":18,"scrollback":2000,"cursor_style":"underline","cursor_blink":false},"events":{"time_format":"h12","default_filter":"security"}}"#;
    account.plant(old);
    let service = account.service();
    let view = service.read();
    assert_eq!(view.storage, Storage::Ready);
    assert_eq!(view.preferences.schema_version, 3);
    assert_eq!(view.preferences.interface.landing_page, LandingPage::Events);
    assert_eq!(view.preferences.interface.theme, AppearanceTheme::Light);
    assert_eq!(view.preferences.interface.language, Language::En);
    assert_eq!(view.preferences.terminal.font_size, 18);
    // Reading migrates in memory only: the stored bytes are the owner's until a change.
    assert_eq!(fs::read(account.file()).unwrap(), old);

    let mut upgraded = view.preferences;
    upgraded.interface.language = Language::Es;
    service.update(upgraded).unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&fs::read(account.file()).unwrap()).unwrap();
    assert_eq!(saved["schema_version"], 3);
    assert_eq!(saved["interface"]["language"], "es");
    assert_eq!(saved["interface"]["theme"], "light");
    assert_eq!(account.service().read().preferences, upgraded);
}

#[test]
fn schema_two_with_a_language_it_never_had_is_preserved_as_recovered_data() {
    let account = Account::new();
    let odd = br#"{"schema_version":2,"interface":{"landing_page":"console","theme":"dark","language":"es"},"terminal":{"font_size":16,"scrollback":5000,"cursor_style":"block","cursor_blink":false},"events":{"time_format":"system","default_filter":"all"}}"#;
    account.plant(odd);
    let service = account.service();
    assert_eq!(service.read().storage, Storage::Recovered);
    assert_eq!(fs::read(account.rejected()).unwrap(), odd);
}

#[test]
fn schema_one_with_unrecognized_fields_is_preserved_as_recovered_data() {
    let account = Account::new();
    let old_with_extra = br#"{"schema_version":1,"interface":{"landing_page":"console","auto_theme":true},"terminal":{"font_size":16,"scrollback":5000,"cursor_style":"block","cursor_blink":false},"events":{"time_format":"system","default_filter":"all"}}"#;
    account.plant(old_with_extra);
    let service = account.service();
    assert_eq!(service.read().storage, Storage::Recovered);
    assert_eq!(fs::read(account.rejected()).unwrap(), old_with_extra);
}

#[test]
fn the_file_and_its_directory_are_private() {
    let account = Account::new();
    account.service().update(changed()).unwrap();
    let dir = fs::metadata(account.dir()).unwrap();
    let file = fs::metadata(account.file()).unwrap();
    assert_eq!(dir.mode() & 0o777, 0o700);
    assert_eq!(file.mode() & 0o777, 0o600);
    assert_eq!(file.nlink(), 1);
    assert!(file.len() <= MAX_FILE);
    // A write leaves the one file, and no temporary one.
    assert_eq!(account.names(), ["preferences.json"]);
}

#[test]
fn the_saved_file_is_versioned_and_holds_nothing_but_preferences() {
    let account = Account::new();
    account.service().update(changed()).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(account.file()).unwrap()).unwrap();
    assert_eq!(value["schema_version"], 3);
    assert_eq!(value, serde_json::to_value(changed()).unwrap());
}

#[test]
fn an_unchanged_set_is_not_written_again() {
    let account = Account::new();
    let service = account.service();
    service.update(changed()).unwrap();
    let before = fs::metadata(account.file()).unwrap().ino();
    service.update(changed()).unwrap();
    assert_eq!(fs::metadata(account.file()).unwrap().ino(), before);
}

#[test]
fn a_directory_that_does_not_exist_yet_is_created_private_by_the_first_save() {
    let account = Account::new();
    fs::remove_dir(account.root.join("config")).unwrap();
    let service = account.service();
    assert_eq!(service.read().storage, Storage::Ready);
    service.update(changed()).unwrap();
    assert_eq!(fs::metadata(account.dir()).unwrap().mode() & 0o777, 0o700);
    assert_eq!(account.service().read().preferences, changed());
}

// ---------------------------------------------------------------- bounds

#[test]
fn every_bound_is_enforced_by_rust() {
    let account = Account::new();
    let service = account.service();
    for (font, scrollback, ok) in [
        (9u8, 5000u32, false),
        (10, 5000, true),
        (24, 5000, true),
        (25, 5000, false),
        (0, 5000, false),
        (255, 5000, false),
        (16, 499, false),
        (16, 500, true),
        (16, 50_000, true),
        (16, 50_001, false),
        (16, u32::MAX, false),
    ] {
        let mut next = Preferences::default();
        next.terminal.font_size = font;
        next.terminal.scrollback = scrollback;
        let before = service.read();
        let result = service.update(next);
        if ok {
            assert_eq!(result.unwrap().preferences, next, "{font}/{scrollback}");
        } else {
            assert_eq!(
                result.unwrap_err().code(),
                SettingsFailure::Invalid,
                "{font}/{scrollback}"
            );
            assert_eq!(service.read(), before, "a refused update changes nothing");
        }
    }
}

#[test]
fn an_unsupported_set_version_is_refused() {
    let account = Account::new();
    let service = account.service();
    for version in [0, 2, 4, u32::MAX] {
        let next = Preferences {
            schema_version: version,
            ..Preferences::default()
        };
        assert_eq!(
            service.update(next).unwrap_err().code(),
            SettingsFailure::Invalid
        );
    }
    assert!(!account.dir().exists());
}

// ---------------------------------------------------------------- malformed files

#[test]
fn every_kind_of_damaged_file_falls_back_to_defaults_and_is_set_aside() {
    let big = vec![b' '; (MAX_FILE + 1) as usize];
    let mut out_of_range = serde_json::to_value(Preferences::default()).unwrap();
    out_of_range["terminal"]["font_size"] = 99.into();
    let mut unknown = serde_json::to_value(Preferences::default()).unwrap();
    unknown["terminal"]["shell"] = "/bin/sh".into();
    let mut word = serde_json::to_value(Preferences::default()).unwrap();
    word["interface"]["landing_page"] = "settings".into();
    let mut missing = serde_json::to_value(Preferences::default()).unwrap();
    missing.as_object_mut().unwrap().remove("terminal");
    let mut missing_theme = serde_json::to_value(Preferences::default()).unwrap();
    missing_theme["interface"]
        .as_object_mut()
        .unwrap()
        .remove("theme");
    let mut version = serde_json::to_value(Preferences::default()).unwrap();
    version["schema_version"] = "1".into();
    let mut zero = serde_json::to_value(Preferences::default()).unwrap();
    zero["schema_version"] = 0.into();
    let mut top = serde_json::to_value(Preferences::default()).unwrap();
    top["auto_approve"] = true.into();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("garbage", b"\x00\xff\xfe not json".to_vec()),
        ("truncated", br#"{"schema_version":1,"inter"#.to_vec()),
        ("array", b"[1,2,3]".to_vec()),
        ("null", b"null".to_vec()),
        ("number", b"7".to_vec()),
        (
            "trailing",
            format!(
                "{} x",
                String::from_utf8(json(&Preferences::default())).unwrap()
            )
            .into_bytes(),
        ),
        ("oversized", big),
        ("out of range", serde_json::to_vec(&out_of_range).unwrap()),
        ("unknown field", serde_json::to_vec(&unknown).unwrap()),
        ("unknown top field", serde_json::to_vec(&top).unwrap()),
        ("unknown word", serde_json::to_vec(&word).unwrap()),
        ("missing section", serde_json::to_vec(&missing).unwrap()),
        (
            "missing schema v2 theme",
            serde_json::to_vec(&missing_theme).unwrap(),
        ),
        ("version as text", serde_json::to_vec(&version).unwrap()),
        ("version zero", serde_json::to_vec(&zero).unwrap()),
        (
            "no version",
            br#"{"interface":{"landing_page":"console"}}"#.to_vec(),
        ),
    ];
    for (name, bytes) in cases {
        let account = Account::new();
        account.plant(&bytes);
        let service = account.service();
        let view = service.read();
        assert_eq!(view.storage, Storage::Recovered, "{name}");
        assert_eq!(view.preferences, Preferences::default(), "{name}");
        // Non-destructive: the file was moved, not read into anything and not deleted.
        assert!(!account.file().exists(), "{name}");
        assert_eq!(fs::read(account.rejected()).unwrap(), bytes, "{name}");
        // The next change is saved and the notice is gone.
        let view = service.update(changed()).unwrap();
        assert_eq!(view.storage, Storage::Ready, "{name}");
        assert_eq!(account.service().read().preferences, changed(), "{name}");
    }
}

// ---------------------------------------------------------------- newer files

#[test]
fn a_newer_schema_is_neither_reinterpreted_nor_overwritten() {
    let account = Account::new();
    let future = br#"{"schema_version":4,"interface":{"landing_page":"console","tabs":[1,2]},"unknown":{"a":1}}"#;
    account.plant(future);
    let service = account.service();
    let view = service.read();
    assert_eq!(view.storage, Storage::Newer);
    assert_eq!(view.preferences, Preferences::default());
    assert_eq!(fs::read(account.file()).unwrap(), future);
    // Read only: changing preferences would destroy what the newer version saved.
    assert_eq!(
        service.update(changed()).unwrap_err().code(),
        SettingsFailure::ReadOnly
    );
    assert_eq!(fs::read(account.file()).unwrap(), future);
    assert!(!account.rejected().exists());
    // Reset is the explicit way out.
    let view = service.reset().unwrap();
    assert_eq!(view.storage, Storage::Ready);
    assert!(!account.file().exists());
    assert_eq!(service.update(changed()).unwrap().preferences, changed());
}

#[test]
fn a_much_newer_schema_number_is_still_just_newer() {
    let account = Account::new();
    account.plant(br#"{"schema_version":18446744073709551615}"#);
    assert_eq!(account.service().read().storage, Storage::Newer);
}

// ---------------------------------------------------------------- unsafe storage

#[test]
fn a_file_others_can_read_or_write_is_not_read_at_all() {
    for mode in [0o644, 0o640, 0o666, 0o660, 0o400, 0o700] {
        let account = Account::new();
        // Content that would be accepted if it were read: a different font size shows it.
        account.plant(&json(&changed()));
        fs::set_permissions(account.file(), fs::Permissions::from_mode(mode)).unwrap();
        let service = account.service();
        let view = service.read();
        assert_eq!(view.storage, Storage::Unsafe, "{mode:o}");
        assert_eq!(view.preferences, Preferences::default(), "{mode:o}");
        // It was not moved either.
        assert!(account.file().exists(), "{mode:o}");
        assert!(!account.rejected().exists(), "{mode:o}");
        // A change replaces it with a private file.
        service.update(changed()).unwrap();
        assert_eq!(fs::metadata(account.file()).unwrap().mode() & 0o777, 0o600);
        assert_eq!(account.service().read().storage, Storage::Ready);
    }
}

#[test]
fn a_symlinked_file_is_never_followed_read_or_written() {
    let account = Account::new();
    let target = account.root.join("elsewhere.json");
    fs::write(&target, json(&changed())).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    fs::create_dir_all(account.dir()).unwrap();
    fs::set_permissions(account.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&target, account.file()).unwrap();
    let service = account.service();
    let view = service.read();
    assert_eq!(view.storage, Storage::Unsafe);
    assert_eq!(
        view.preferences,
        Preferences::default(),
        "the link was followed"
    );
    // Saving replaces the link; what it pointed at is not written through.
    let before = fs::read(&target).unwrap();
    let mut other = Preferences::default();
    other.terminal.font_size = 12;
    service.update(other).unwrap();
    assert_eq!(fs::read(&target).unwrap(), before);
    assert!(!fs::symlink_metadata(account.file()).unwrap().is_symlink());
    assert_eq!(account.service().read().preferences, other);
}

#[test]
fn a_reset_removes_a_link_and_not_what_it_points_at() {
    let account = Account::new();
    let target = account.root.join("precious");
    fs::write(&target, b"keep").unwrap();
    fs::create_dir_all(account.dir()).unwrap();
    fs::set_permissions(account.dir(), fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&target, account.file()).unwrap();
    account.service().reset().unwrap();
    assert!(fs::symlink_metadata(account.file()).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"keep");
}

#[test]
fn a_second_name_for_the_file_is_not_trusted() {
    let account = Account::new();
    account.plant(&json(&changed()));
    fs::hard_link(account.file(), account.root.join("alias")).unwrap();
    assert_eq!(account.service().read().storage, Storage::Unsafe);
}

#[test]
fn a_file_that_is_a_directory_or_a_pipe_is_unsafe_not_a_crash() {
    let account = Account::new();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(account.dir())
        .unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(account.file())
        .unwrap();
    assert_eq!(account.service().read().storage, Storage::Unsafe);
}

#[test]
fn a_directory_others_can_enter_is_unavailable_and_left_alone() {
    for mode in [0o755, 0o750, 0o770, 0o777, 0o500] {
        let account = Account::new();
        account.plant(&json(&changed()));
        fs::set_permissions(account.dir(), fs::Permissions::from_mode(mode)).unwrap();
        let service = account.service();
        let view = service.read();
        assert_eq!(view.storage, Storage::Unavailable, "{mode:o}");
        assert_eq!(view.preferences, Preferences::default(), "{mode:o}");
        assert_eq!(
            service.update(changed()).unwrap_err().code(),
            SettingsFailure::NotSaved,
            "{mode:o}"
        );
        assert_eq!(
            service.reset().unwrap_err().code(),
            SettingsFailure::NotSaved,
            "{mode:o}"
        );
        // Nothing in it was touched.
        fs::set_permissions(account.dir(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            fs::read(account.file()).unwrap(),
            json(&changed()),
            "{mode:o}"
        );
        assert_eq!(account.names(), ["preferences.json"], "{mode:o}");
    }
}

#[test]
fn a_directory_that_is_a_link_is_unavailable() {
    let account = Account::new();
    let elsewhere = account.root.join("elsewhere");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&elsewhere)
        .unwrap();
    symlink(&elsewhere, account.dir()).unwrap();
    let service = account.service();
    assert_eq!(service.read().storage, Storage::Unavailable);
    assert_eq!(
        service.update(changed()).unwrap_err().code(),
        SettingsFailure::NotSaved
    );
    assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[test]
fn without_a_store_the_defaults_apply_and_nothing_is_saved() {
    let service = SettingsService::open(None);
    assert_eq!(service.read().storage, Storage::Unavailable);
    assert_eq!(service.read().preferences, Preferences::default());
    assert_eq!(
        service.update(changed()).unwrap_err().code(),
        SettingsFailure::NotSaved
    );
    assert_eq!(service.reset().unwrap().storage, Storage::Unavailable);
}

#[test]
fn a_store_needs_a_plain_absolute_path() {
    assert!(Store::at(PathBuf::from("relative/config")).is_none());
    assert!(Store::at(PathBuf::from("/tmp/../etc")).is_none());
    assert!(Store::at(PathBuf::from("/tmp/bad\nname")).is_none());
    assert!(Store::at(PathBuf::from("/tmp/good")).is_some());
}

// ---------------------------------------------------------------- atomic writes

#[test]
fn a_write_interrupted_before_the_rename_changes_nothing() {
    let account = Account::new();
    let service = account.service();
    service.update(changed()).unwrap();
    let saved = fs::read(account.file()).unwrap();
    let inode = fs::metadata(account.file()).unwrap().ino();
    let mut other = Preferences::default();
    other.terminal.font_size = 11;
    let interrupted = account
        .store()
        .save_with(&other, || Err(io::Error::other("power cut")));
    assert_eq!(interrupted, Err(StoreError::Io));
    // The live file is the old one, whole, and no temporary file is left.
    assert_eq!(fs::read(account.file()).unwrap(), saved);
    assert_eq!(fs::metadata(account.file()).unwrap().ino(), inode);
    assert_eq!(account.names(), ["preferences.json"]);
    assert_eq!(account.service().read().preferences, changed());
}

#[test]
fn a_successful_write_replaces_the_file_and_never_truncates_it_in_place() {
    let account = Account::new();
    let service = account.service();
    service.update(changed()).unwrap();
    // A reader holding the old file open keeps reading the old, whole content.
    let old = fs::File::open(account.file()).unwrap();
    let before = fs::metadata(account.file()).unwrap().ino();
    let mut next = changed();
    next.terminal.font_size = 22;
    service.update(next).unwrap();
    assert_ne!(fs::metadata(account.file()).unwrap().ino(), before);
    let mut seen = Vec::new();
    io::Read::read_to_end(&mut &old, &mut seen).unwrap();
    assert_eq!(seen, {
        let mut bytes = json(&changed());
        bytes.push(b'\n');
        bytes
    });
}

#[test]
fn what_an_interrupted_writer_left_is_ignored_and_cleaned() {
    let account = Account::new();
    account.plant(&json(&changed()));
    // A writer that died after creating its temporary file and before renaming it.
    let leftovers = [".preferences.4242.0.tmp", ".preferences.1.17.tmp"];
    for name in leftovers {
        let path = account.dir().join(name);
        fs::write(&path, br#"{"schema_version":1,"inter"#).unwrap();
        // Long dead: a minute is far more than a write takes.
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(600))
            .unwrap();
    }
    // A temporary file of a write that is going on right now, in another desktop.
    let live = account.dir().join(".preferences.9999.0.tmp");
    fs::write(&live, b"in flight").unwrap();
    // Things that only look like leftovers are not this module's to delete.
    let foreign = [
        ".preferences.x.0.tmp",
        ".preferences.1.tmp",
        "notes.tmp",
        ".preferences..tmp",
    ];
    for name in foreign {
        fs::write(account.dir().join(name), b"mine").unwrap();
    }
    let view = account.service().read();
    assert_eq!(view.storage, Storage::Ready);
    assert_eq!(view.preferences, changed(), "a partial write was read");
    for name in leftovers {
        assert!(!account.dir().join(name).exists(), "{name}");
    }
    for name in foreign {
        assert_eq!(
            fs::read(account.dir().join(name)).unwrap(),
            b"mine",
            "{name}"
        );
    }
    assert_eq!(
        fs::read(live).unwrap(),
        b"in flight",
        "a write in flight was removed"
    );
}

#[test]
fn a_stale_temporary_file_never_blocks_a_save() {
    let account = Account::new();
    account.plant(&json(&Preferences::default()));
    fs::write(
        account
            .dir()
            .join(format!(".preferences.{}.0.tmp", std::process::id())),
        b"stale",
    )
    .unwrap();
    // The counter is process wide, so the stale name may or may not be the next one.
    for _ in 0..3 {
        account.store().save(&changed()).unwrap();
    }
    assert_eq!(account.service().read().preferences, changed());
}

// ---------------------------------------------------------------- reset

#[test]
fn reset_removes_preferences_and_nothing_else() {
    let account = Account::new();
    let service = account.service();
    service.update(changed()).unwrap();
    // Everything that stands for the rest of the product, around the preference file.
    let sessions = account.root.join("config/wyss-console-sessions");
    fs::write(&sessions, b"sessions").unwrap();
    let unit = account.root.join("config/systemd");
    fs::create_dir_all(&unit).unwrap();
    fs::write(unit.join("wyss-control.service"), b"unit").unwrap();
    fs::write(account.dir().join("preferences.invalid.json"), b"old").unwrap();
    let view = service.reset().unwrap();
    assert_eq!(view.preferences, Preferences::default());
    assert_eq!(view.storage, Storage::Ready);
    assert!(account.names().is_empty(), "{:?}", account.names());
    assert_eq!(fs::read(&sessions).unwrap(), b"sessions");
    assert_eq!(
        fs::read(unit.join("wyss-control.service")).unwrap(),
        b"unit"
    );
    account.canonical_untouched();
    // And the defaults are what a restart finds.
    assert_eq!(account.service().read().preferences, Preferences::default());
}

#[test]
fn reset_with_nothing_saved_is_a_quiet_success() {
    let account = Account::new();
    let view = account.service().reset().unwrap();
    assert_eq!(view.preferences, Preferences::default());
    assert!(!account.dir().exists());
}

#[test]
fn reset_clears_every_notice() {
    for (bytes, expected) in [
        (&b"garbage"[..], Storage::Recovered),
        (&br#"{"schema_version":9}"#[..], Storage::Newer),
    ] {
        let account = Account::new();
        account.plant(bytes);
        let service = account.service();
        assert_eq!(service.read().storage, expected);
        assert_eq!(service.reset().unwrap().storage, Storage::Ready);
        assert!(account.names().is_empty());
    }
}

// ---------------------------------------------------------------- concurrency

#[test]
fn many_rapid_updates_leave_the_last_one_whole() {
    let account = Account::new();
    let service = account.service();
    for step in 0..200u32 {
        let mut next = Preferences::default();
        next.terminal.font_size = 10 + (step % 15) as u8;
        next.terminal.scrollback = 500 + step;
        service.update(next).unwrap();
    }
    let last = service.read().preferences;
    assert_eq!(last.terminal.scrollback, 699);
    assert_eq!(account.service().read().preferences, last);
    assert_eq!(account.names(), ["preferences.json"]);
}

#[test]
fn concurrent_updates_never_corrupt_the_file() {
    let account = Account::new();
    let service = account.service();
    let start = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8u32)
        .map(|worker| {
            let (service, start) = (service.clone(), start.clone());
            thread::spawn(move || {
                start.wait();
                for step in 0..40u32 {
                    let mut next = Preferences::default();
                    next.terminal.scrollback = 500 + worker * 100 + step;
                    service.update(next).unwrap();
                    // Every read in between is a whole, valid set.
                    assert!(service.read().preferences.validate().is_ok());
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let in_force = service.read().preferences;
    assert_eq!(account.service().read().preferences, in_force);
    assert_eq!(account.names(), ["preferences.json"]);
}

#[test]
fn a_reader_of_the_file_only_ever_sees_a_whole_set() {
    let account = Account::new();
    let (a, b) = (Preferences::default(), changed());
    account.store().save(&a).unwrap();
    let store = account.store();
    let writer = {
        let store = store.clone();
        thread::spawn(move || {
            for step in 0..200 {
                store.save(if step % 2 == 0 { &b } else { &a }).unwrap();
            }
        })
    };
    for _ in 0..400 {
        match store.load() {
            Loaded::Stored(seen) => assert!(seen == a || seen == b),
            other => panic!("a reader saw {other:?}"),
        }
    }
    writer.join().unwrap();
}

#[test]
fn an_update_racing_a_reset_leaves_one_of_the_two() {
    for round in 0..30 {
        let account = Account::new();
        let service = account.service();
        service.update(Preferences::default()).unwrap();
        let start = Arc::new(Barrier::new(2));
        let update = {
            let (service, start) = (service.clone(), start.clone());
            thread::spawn(move || {
                start.wait();
                service.update(changed()).unwrap();
            })
        };
        let reset = {
            let (service, start) = (service.clone(), start.clone());
            thread::spawn(move || {
                start.wait();
                service.reset().unwrap();
            })
        };
        update.join().unwrap();
        reset.join().unwrap();
        let in_force = service.read();
        assert_eq!(in_force.storage, Storage::Ready, "round {round}");
        assert!(
            in_force.preferences == changed() || in_force.preferences == Preferences::default(),
            "round {round}"
        );
        // What is in force is what a restart finds: memory and disk never disagree.
        assert_eq!(
            account.service().read().preferences,
            in_force.preferences,
            "round {round}"
        );
        assert!(
            account.names().iter().all(|n| n == "preferences.json"),
            "{:?}",
            account.names()
        );
    }
}

// ---------------------------------------------------------------- about

#[test]
fn about_is_made_of_build_constants() {
    let about = About::of_build();
    assert_eq!(about.version, env!("CARGO_PKG_VERSION"));
    assert!(about.platform.starts_with("linux-"));
    if let Some(revision) = about.revision {
        assert!((7..=40).contains(&revision.len()));
        assert!(
            revision
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        );
    }
    // No path, no user name and no environment can be in it.
    assert!(!serde_json::to_string(&about).unwrap().contains('/'));
}
