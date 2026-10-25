//! The exact builds this adapter is written against. A build that is not in this table is
//! refused before any process starts; there is no "close enough" and no downgrade path.
//!
//! The pair comes from the official Python distribution `claude-agent-sdk`, which bundles the
//! Claude Code CLI it was released with. The digest is of that bundled executable, so a
//! replaced or patched binary with the same version string is refused too.

/// Version of the bridge protocol this adapter speaks.
pub const BRIDGE_PROTOCOL: u8 = 1;
/// Largest frame in either direction (also the bridge's own limit).
pub const MAX_FRAME: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildPin {
    pub sdk_version: &'static str,
    pub cli_version: &'static str,
    /// `linux-x86_64` is the only platform with accepted evidence.
    pub platform: &'static str,
    /// SHA-256 of the bundled CLI executable, lowercase hex.
    pub cli_sha256: &'static str,
}

pub const BUILDS: [BuildPin; 1] = [BuildPin {
    sdk_version: "0.2.173",
    cli_version: "2.1.309",
    platform: "linux-x86_64",
    cli_sha256: "4d704a8aecbe038affa3000f498ae17c1b4f753add8facb8764120affc99b658",
}];

/// `profiles.frozen_digest` of each profile: the option set, the tool allow-list and the CLI
/// environment the bridge builds, without per-session values (cwd, resume, model, effort).
/// `f8::claude` tests recompute it from the embedded bridge source with Python, so a change
/// to `claude-bridge/profiles.py` that is not mirrored here fails.
pub const INFERENCE_OPTIONS_DIGEST: &str =
    "d2bf179435df6ca9b9b2388b9979bf2eab253712184b50c765e2265ed4f596eb";
pub const LOCAL_TOOLS_OPTIONS_DIGEST: &str =
    "626480c0636fbf828f38aaca2056bc1206201bf460a19f250f495f6edcebe078";

pub fn build_for(sdk: &str, cli: &str) -> Option<&'static BuildPin> {
    BUILDS
        .iter()
        .find(|b| b.sdk_version == sdk && b.cli_version == cli)
}

/// Whether a lowercase hex SHA-256 string is well formed (the pin is data, not trust).
pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pin_is_well_formed_and_unique() {
        for b in BUILDS {
            assert!(is_sha256_hex(b.cli_sha256));
            assert!(
                b.sdk_version
                    .split('.')
                    .all(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
            );
        }
        let mut seen = std::collections::BTreeSet::new();
        assert!(
            BUILDS
                .iter()
                .all(|b| seen.insert((b.sdk_version, b.platform)))
        );
    }

    #[test]
    fn only_the_exact_pair_is_a_build() {
        assert!(build_for("0.2.173", "2.1.309").is_some());
        for (sdk, cli) in [
            ("0.2.172", "2.1.309"),
            ("0.2.173", "2.1.308"),
            ("0.2.173", ""),
            ("", "2.1.309"),
            ("0.2.173 ", "2.1.309"),
            ("0.3.308", "2.1.308"),
        ] {
            assert!(build_for(sdk, cli).is_none(), "{sdk} {cli}");
        }
    }

    #[test]
    fn sha_check_is_strict() {
        assert!(!is_sha256_hex(&"A".repeat(64)));
        assert!(!is_sha256_hex(&"a".repeat(63)));
        assert!(!is_sha256_hex(&"g".repeat(64)));
    }
}
