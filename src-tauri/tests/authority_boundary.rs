//! TS-SEC-03, the half a source scan cannot give: the authority binary that was built
//! does not contain the configuration module (C-02). The module's name is in every path
//! and symbol it would add; its tests are the other half (`agentconfig::tests`).
use std::fs;

#[test]
fn the_built_authority_does_not_contain_the_configuration_module() {
    let bytes = fs::read(env!("CARGO_BIN_EXE_wyss-control")).unwrap();
    assert!(bytes.len() > 1_000_000, "not the authority binary");
    // Spelled in two halves, so that this file's own strings are not what is found.
    let name = concat!("agent", "config");
    assert!(
        !bytes
            .windows(name.len())
            .any(|window| window == name.as_bytes()),
        "the authority binary names {name}"
    );
    // Control: the search can find a name that is there.
    let control = "wyss_control";
    assert!(
        bytes
            .windows(control.len())
            .any(|window| window == control.as_bytes()),
        "the search found nothing in the authority binary"
    );
}
