use super::*;

fn parse(args: &[&str]) -> ControlResult<Serve> {
    Serve::parse(args.iter().map(OsString::from))
}

#[test]
fn the_three_explicit_parts_are_all_it_needs() {
    let serve = parse(&[
        "--protocol",
        "2",
        "--runtime-root",
        "/run/x",
        "--state-root",
        "/state/x",
    ])
    .unwrap();
    assert_eq!(serve.runtime_root, PathBuf::from("/run/x"));
    assert_eq!(serve.state_root, PathBuf::from("/state/x"));
    assert_eq!(serve.image, Image::Existing);
}

#[test]
fn an_image_is_created_or_migrated_only_by_name() {
    let base = [
        "--protocol",
        "2",
        "--runtime-root",
        "/r",
        "--state-root",
        "/s",
    ];
    let with = |extra: &[&str]| parse(&[&base[..], extra].concat());
    assert_eq!(with(&["--create"]).unwrap().image, Image::Create);
    assert_eq!(
        with(&["--migrate-copy", "/c/copy.sqlite3"]).unwrap().image,
        Image::MigrateCopy(PathBuf::from("/c/copy.sqlite3"))
    );
    assert_eq!(
        with(&["--create", "--migrate-copy", "/c"]),
        Err(E::InvalidRequest)
    );
    assert_eq!(with(&["--migrate-copy"]), Err(E::InvalidRequest));
    assert_eq!(with(&["--migrate-copy", "relative"]), Err(E::UnsafePath));
}

#[test]
fn nothing_but_version_two_with_both_directories_is_accepted() {
    for args in [
        &[][..],
        &["--protocol", "2"],
        &[
            "--protocol",
            "1",
            "--runtime-root",
            "/r",
            "--state-root",
            "/s",
        ],
        &["--protocol", "2", "--runtime-root", "/r"],
        &[
            "--protocol",
            "2",
            "--runtime-root",
            "r",
            "--state-root",
            "/s",
        ],
        &[
            "--protocol",
            "2",
            "--runtime-root",
            "/r",
            "--state-root",
            "/s",
            "x",
        ],
        &[
            "--protocol",
            "2",
            "--runtime-root",
            "/r",
            "--runtime-root",
            "/r2",
            "--state-root",
            "/s",
        ],
    ] {
        assert!(parse(args).is_err(), "{args:?}");
    }
}

#[test]
fn a_path_too_long_for_a_socket_is_refused_before_anything_is_made() {
    let long = format!("/{}", "a".repeat(120));
    assert_eq!(
        parse(&[
            "--protocol",
            "2",
            "--runtime-root",
            &long,
            "--state-root",
            "/s"
        ]),
        Err(E::UnsafePath)
    );
}
