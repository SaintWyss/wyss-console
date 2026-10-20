//! Remote grants (16 §5.3): a closed vocabulary, the profiles that bundle them, the step-up each
//! one needs and how long each may live (GR2). Nothing here is read from a file or a message: a
//! grant a device names that is not in [`Grant`] is not a grant.
use serde::{Deserialize, Serialize};

/// What a device may do, as one word each. **`TerminalWrite` is the user's whole account**
/// (16 §5.3): every restriction on the other grants is void for a device that also holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grant {
    SessionsRead,
    ManagedRead,
    ApprovalsRespond,
    ManagedTurns,
    TerminalRead,
    TerminalWrite,
    ConfigRead,
    ConfigRequest,
    VaultMetadata,
    DevicesRevoke,
}

impl Grant {
    pub const ALL: [Self; 10] = [
        Self::SessionsRead,
        Self::ManagedRead,
        Self::ApprovalsRespond,
        Self::ManagedTurns,
        Self::TerminalRead,
        Self::TerminalWrite,
        Self::ConfigRead,
        Self::ConfigRequest,
        Self::VaultMetadata,
        Self::DevicesRevoke,
    ];

    /// Whether a request under this grant needs a fresh signature of the device's presence key
    /// (the "Step-up" column of 16 §5.3): every response, claim, start, request and revocation.
    pub const fn needs_step_up(self) -> bool {
        matches!(
            self,
            Self::ApprovalsRespond
                | Self::ManagedTurns
                | Self::TerminalWrite
                | Self::ConfigRequest
                | Self::DevicesRevoke
        )
    }

    /// The longest a grant may be given for (GR2). `None` means **no grant of this kind may be
    /// created yet**: the roadmap leaves it to a decision that has not been made.
    pub const fn longest_days(self) -> Option<u32> {
        match self {
            // Metadata: the read/watch tier of GR2.
            Self::SessionsRead | Self::ManagedRead => Some(90),
            // What a terminal printed, secrets included: a shorter tier.
            Self::TerminalRead => Some(30),
            // The non-shell grants "subject to review": the cautious end of the tier until then.
            Self::ApprovalsRespond
            | Self::ConfigRead
            | Self::VaultMetadata
            | Self::DevicesRevoke => Some(30),
            Self::ConfigRequest => Some(7),
            // A day of eligibility to ask for a claim is not "unattended shell": the claim itself is
            // the thing, 15 minutes at most and only after step-up (see `INPUT_CLAIM_MINUTES`).
            Self::ManagedTurns => Some(7),
            // GR2: the 7-day `terminal.write` grant is NOT accepted, and no LP4 lifetime is accepted
            // before DN-44. So none can be created.
            Self::TerminalWrite => None,
        }
    }
}

/// The longest an exclusive input claim lives (a planning candidate, GR2).
pub const INPUT_CLAIM_MINUTES: u32 = 15;

/// The least-privilege bundles of 16 §5.3. LP4 is listed so that it is named, and cannot be given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Watch: the default for a new device.
    Lp1,
    /// Approve.
    Lp2,
    /// Read terminals.
    Lp3,
    /// Full control: the user's whole account. Not offered before DN-44.
    Lp4,
}

impl Profile {
    pub fn grants(self) -> &'static [Grant] {
        match self {
            Self::Lp1 => &[Grant::SessionsRead, Grant::ManagedRead],
            Self::Lp2 => &[
                Grant::SessionsRead,
                Grant::ManagedRead,
                Grant::ApprovalsRespond,
            ],
            Self::Lp3 => &[Grant::SessionsRead, Grant::ManagedRead, Grant::TerminalRead],
            Self::Lp4 => &[
                Grant::SessionsRead,
                Grant::ManagedRead,
                Grant::TerminalRead,
                Grant::TerminalWrite,
                Grant::ManagedTurns,
                Grant::ApprovalsRespond,
            ],
        }
    }
}

/// Why a grant cannot be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantRefused {
    /// Longer than its kind may be given for.
    TooLong,
    /// A kind that needs a decision before any may exist (`TerminalWrite`, hence LP4).
    NeedsDecision,
    /// An expiry that is not in the future.
    NotInTheFuture,
}

/// One grant of one device, with the instant it ends (Unix seconds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    pub grant: Grant,
    pub expires_at: u64,
}

pub const DAY: u64 = 86_400;

/// Makes a grant for `days` from `now`, if its kind permits that long.
pub fn give(grant: Grant, now: u64, days: u32) -> Result<Held, GrantRefused> {
    let longest = grant.longest_days().ok_or(GrantRefused::NeedsDecision)?;
    if days == 0 {
        return Err(GrantRefused::NotInTheFuture);
    }
    if days > longest {
        return Err(GrantRefused::TooLong);
    }
    Ok(Held {
        grant,
        expires_at: now.saturating_add(u64::from(days) * DAY),
    })
}

/// The grants of a profile, each for its own longest permitted time capped at `days`. A profile
/// with a kind that cannot be given (LP4) is refused whole, never given in part.
pub fn give_profile(profile: Profile, now: u64, days: u32) -> Result<Vec<Held>, GrantRefused> {
    profile
        .grants()
        .iter()
        .map(|grant| {
            let capped = days.min(grant.longest_days().ok_or(GrantRefused::NeedsDecision)?);
            give(*grant, now, capped)
        })
        .collect()
}

/// A grant is `expiring` within three days of its end (SM-GRANT) and `expired` from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantState {
    Active,
    Expiring,
    Expired,
}
pub fn state_of(held: &Held, now: u64) -> GrantState {
    if now >= held.expires_at {
        GrantState::Expired
    } else if held.expires_at - now <= 3 * DAY {
        GrantState::Expiring
    } else {
        GrantState::Active
    }
}
