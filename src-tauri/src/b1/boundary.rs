//! The approval-boundary contract (ADR 0058, roadmap M5 track b): what has to be true of a channel
//! before an approval carried over it can be called *strong*, the facts of the channel that exists
//! today, and the gates that stay closed until an independent security review passes the OS trust
//! boundary this needs.
//!
//! Today's approval path (F8) is a same-user Unix socket checked with `SO_PEERCRED`: any process of
//! the same user passes the check. That is the accepted U boundary (ADR 0011/0019/0021), honest and
//! useful against malformed or stale requests, and **not** protection against an unrestricted
//! process of the account. B1 does not change that for U; what B1 adds is a place where an
//! untrusted worker runs *without* a route to that socket (its mount, pid and network namespaces
//! do not contain it; `tests/b1_process.rs` observes each, against a baseline that shows the same
//! probe reaching it without B1). A stronger approval needs a principal the worker cannot be, which
//! needs privileges this change does not have and does not ask for.
//!
//! What this module is: the *statement* of the requirements, the facts of today's channel, four
//! constants that are `false`, and a regression check that fails if a named set of surfaces starts to
//! mention the B1 module. It is not an enforcement point: nothing here decides an approval, and the
//! source scan covers the surfaces it names, not every route a future change could add. When a
//! stronger mode is built, the permission has to be checked where the effect happens.

/// The trust lanes of the product (architecture §4). B1 is a *different* contract from U and M.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    /// Native unrestricted: the user's own rights, provider controls, not Wyss mediation.
    U,
    /// Restricted staged worker: pending results only, no ambient authority.
    B1,
    /// Managed: attested surface, per-action authority mediation.
    M,
    /// Planner: proposals are untrusted data.
    P,
    /// Approval authority: human approval integrity against defined attackers.
    A,
}

/// Facts about a channel that carries an approval. A claim of strength needs every one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelFacts {
    /// The same Unix account that the worker/Native process runs as can open the channel.
    pub reachable_by_same_uid: bool,
    /// An untrusted worker (B1) has a route to the channel (a mount, a socket, a descriptor).
    pub reachable_from_confined_worker: bool,
    /// The approver is a principal other than the one the untrusted code runs as.
    pub distinct_principal: bool,
    /// A human act is proven on the approver's side (not a message anyone can compose).
    pub human_presence_proven: bool,
    /// The OS boundary that makes the above true passed an independent security review.
    pub boundary_reviewed: bool,
}

/// Why a channel is not strong. Closed words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weakness {
    SameUidCanConnect,
    ConfinedWorkerCanReach,
    NoDistinctPrincipal,
    NoHumanPresenceProof,
    BoundaryNotReviewed,
}

impl Weakness {
    pub fn word(self) -> &'static str {
        match self {
            Self::SameUidCanConnect => "same_uid_can_connect",
            Self::ConfinedWorkerCanReach => "confined_worker_can_reach",
            Self::NoDistinctPrincipal => "no_distinct_principal",
            Self::NoHumanPresenceProof => "no_human_presence_proof",
            Self::BoundaryNotReviewed => "boundary_not_reviewed",
        }
    }
}

/// Every reason the channel is not strong; empty means strong.
pub fn weaknesses(c: &ChannelFacts) -> Vec<Weakness> {
    let mut w = Vec::new();
    if c.reachable_by_same_uid {
        w.push(Weakness::SameUidCanConnect);
    }
    if c.reachable_from_confined_worker {
        w.push(Weakness::ConfinedWorkerCanReach);
    }
    if !c.distinct_principal {
        w.push(Weakness::NoDistinctPrincipal);
    }
    if !c.human_presence_proven {
        w.push(Weakness::NoHumanPresenceProof);
    }
    if !c.boundary_reviewed {
        w.push(Weakness::BoundaryNotReviewed);
    }
    w
}

pub fn is_strong(c: &ChannelFacts) -> bool {
    weaknesses(c).is_empty()
}

/// The approval path that exists: a same-user socket, checked by `SO_PEERCRED`, answered by a
/// desktop dialog. A confined B1 worker has no route to it, which is a property of the launch plan
/// (observed in the process tests), not of the channel.
pub const TODAY: ChannelFacts = ChannelFacts {
    reachable_by_same_uid: true,
    reachable_from_confined_worker: false,
    distinct_principal: false,
    human_presence_proven: false,
    boundary_reviewed: false,
};

// ---- the gates that stay closed ------------------------------------------------------------

/// A stronger approval mode (a principal other than the account's). Needs privileges and a reviewed
/// OS boundary; opening it is the owner's decision (OD02), after the review.
pub const STRONGER_APPROVAL_MODE: bool = false;
/// Automated Native steps (OL3) with full-account rights. Blocked until the boundary is proven.
pub const NATIVE_AUTOMATED_STEPS: bool = false;
/// Any production launch of a B1 worker. The shim and probe exist only in fixture builds.
pub const B1_PRODUCTION_LAUNCH: bool = false;
/// A planner proposal carrying authority of its own.
pub const PLANNER_EFFECT_AUTHORITY: bool = false;

pub fn gates_closed() -> bool {
    !(STRONGER_APPROVAL_MODE
        || NATIVE_AUTOMATED_STEPS
        || B1_PRODUCTION_LAUNCH
        || PLANNER_EFFECT_AUTHORITY)
}

/// Data from a planner or a Native process, marked as such so that it is not passed around as if it
/// were typed. It can be looked at; the one way out is a validator of the receiving side that can
/// refuse. The type proves nothing by itself: a validator that accepts everything returns the data
/// unchanged, and whether a validator is strict enough is for the code that writes it and its review.
#[derive(Debug)]
pub struct Untrusted<T>(T);

impl<T> Untrusted<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }
    pub fn inspect(&self) -> &T {
        &self.0
    }
    /// The one way out: a validation that can refuse.
    pub fn validate<V, E>(self, check: impl FnOnce(T) -> Result<V, E>) -> Result<V, E> {
        check(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_is_strong_only_when_every_fact_holds() {
        let strong = ChannelFacts {
            reachable_by_same_uid: false,
            reachable_from_confined_worker: false,
            distinct_principal: true,
            human_presence_proven: true,
            boundary_reviewed: true,
        };
        assert!(is_strong(&strong));
        // Each fact, taken away alone, makes it weak and says why.
        let cases: [(ChannelFacts, Weakness); 5] = [
            (
                ChannelFacts {
                    reachable_by_same_uid: true,
                    ..strong
                },
                Weakness::SameUidCanConnect,
            ),
            (
                ChannelFacts {
                    reachable_from_confined_worker: true,
                    ..strong
                },
                Weakness::ConfinedWorkerCanReach,
            ),
            (
                ChannelFacts {
                    distinct_principal: false,
                    ..strong
                },
                Weakness::NoDistinctPrincipal,
            ),
            (
                ChannelFacts {
                    human_presence_proven: false,
                    ..strong
                },
                Weakness::NoHumanPresenceProof,
            ),
            (
                ChannelFacts {
                    boundary_reviewed: false,
                    ..strong
                },
                Weakness::BoundaryNotReviewed,
            ),
        ];
        for (facts, why) in cases {
            assert_eq!(weaknesses(&facts), vec![why], "{}", why.word());
            assert!(!is_strong(&facts));
        }
    }

    #[test]
    fn the_approval_path_that_exists_is_not_called_strong_and_says_all_of_why() {
        assert!(!is_strong(&TODAY));
        let words: Vec<_> = weaknesses(&TODAY).into_iter().map(Weakness::word).collect();
        assert_eq!(
            words,
            [
                "same_uid_can_connect",
                "no_distinct_principal",
                "no_human_presence_proof",
                "boundary_not_reviewed"
            ]
        );
    }

    #[test]
    fn the_gates_are_declared_closed_and_their_declarations_are_checked_in_the_source() {
        assert!(gates_closed());
        // The declarations are `false` in the source. This does not stop another module from adding its
        // own flag; it makes changing these four a visible, reviewed edit.
        let me = include_str!("boundary.rs");
        for gate in [
            "STRONGER_APPROVAL_MODE",
            "NATIVE_AUTOMATED_STEPS",
            "B1_PRODUCTION_LAUNCH",
            "PLANNER_EFFECT_AUTHORITY",
        ] {
            assert!(me.contains(&format!("pub const {gate}: bool = false;")));
        }
    }

    #[test]
    fn the_named_surfaces_do_not_mention_the_b1_module() {
        // These are the surfaces that would make a B1 launch reachable. None names it.
        let scanned: [(&str, &str); 6] = [
            (
                "contracts/control.rs",
                include_str!("../contracts/control.rs"),
            ),
            ("desktop.rs", include_str!("../desktop.rs")),
            ("f8/authority.rs", include_str!("../f8/authority.rs")),
            (
                "f8/claude/launch.rs",
                include_str!("../f8/claude/launch.rs"),
            ),
            ("host/core.rs", include_str!("../host/core.rs")),
            ("control/server.rs", include_str!("../control/server.rs")),
        ];
        for (name, source) in scanned {
            for banned in [
                "crate::b1",
                "wyss_console::b1",
                "b1::plan",
                "b1::confine",
                "wyss-b1",
            ] {
                assert!(!source.contains(banned), "{name} names {banned}");
            }
        }
    }

    #[test]
    fn untrusted_data_can_be_looked_at_and_only_leaves_through_a_validator_that_can_refuse() {
        let proposal = Untrusted::new("rm -rf ~".to_owned());
        assert_eq!(proposal.inspect(), "rm -rf ~");
        let refused: Result<(), &str> = proposal.validate(|_| Err("not typed"));
        assert_eq!(refused, Err("not typed"));
        let ok: Result<usize, ()> = Untrusted::new("abc".to_owned()).validate(|s| Ok(s.len()));
        assert_eq!(ok, Ok(3));
    }
}
