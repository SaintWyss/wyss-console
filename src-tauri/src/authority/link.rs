//! The link's explicit states, why an attempt ends and how long the next one waits.
use crate::contracts::control::ControlError;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    /// Looking for the socket and exchanging the handshake.
    Connecting,
    /// An authority answered; its canonical state and missed events are being read.
    Synchronizing,
    /// The view is canonical as of its head and the durable stream is being watched.
    Connected,
    /// The last attempt ended. The next one starts after a bounded delay.
    Waiting,
    /// The application is closing. Terminal.
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    /// No authority is listening on the runtime socket.
    Absent,
    /// An authority that had answered stopped answering.
    Lost,
    /// An authority answered with a protocol or storage version this build does not speak.
    Incompatible,
    /// The peer broke the protocol: a bad frame, a wrong correlation, a skipped event.
    Malformed,
    /// The runtime directory or socket is not this user's private one.
    UnsafePath,
}
impl Fault {
    /// A failure on a connection whose handshake had completed.
    pub(super) fn of(error: ControlError) -> Self {
        match error {
            ControlError::ProtocolVersionUnsupported | ControlError::StorageIncompatible => {
                Self::Incompatible
            }
            ControlError::UnsafePath | ControlError::PeerRejected => Self::UnsafePath,
            // A frame cut short is the peer going away, as an end of file between frames is.
            ControlError::TransportError
            | ControlError::Timeout
            | ControlError::TruncatedFrame
            | ControlError::StorageUnavailable
            | ControlError::AuthorityUnavailable
            | ControlError::CapacityExceeded
            | ControlError::AlreadyRunning => Self::Lost,
            _ => Self::Malformed,
        }
    }
    /// A failure before any authority answered: nothing was there to lose.
    pub(super) fn connecting(error: ControlError) -> Self {
        match Self::of(error) {
            Self::Lost => Self::Absent,
            other => other,
        }
    }
}

/// What moves the link from one state to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Signal {
    /// The handshake completed.
    Greeted,
    /// A canonical snapshot was read and the stream cursor is in place.
    Synchronized,
    Failed,
    RetryDue,
    Stop,
}
impl Connection {
    /// The only transitions there are. `None` is a transition the link never makes.
    pub(super) fn on(self, signal: Signal) -> Option<Self> {
        match (self, signal) {
            (Self::Stopped, _) => None,
            (_, Signal::Stop) => Some(Self::Stopped),
            (Self::Connecting, Signal::Greeted) => Some(Self::Synchronizing),
            (Self::Synchronizing | Self::Connected, Signal::Synchronized) => Some(Self::Connected),
            (Self::Connecting | Self::Synchronizing | Self::Connected, Signal::Failed) => {
                Some(Self::Waiting)
            }
            (Self::Waiting, Signal::RetryDue) => Some(Self::Connecting),
            _ => None,
        }
    }
}

/// Delay before the next attempt: it doubles from `FIRST` and never exceeds `LIMIT`.
#[derive(Debug, Default)]
pub(super) struct Backoff {
    failures: u32,
}
impl Backoff {
    pub(super) const FIRST: Duration = Duration::from_millis(250);
    pub(super) const LIMIT: Duration = Duration::from_secs(5);
    pub(super) fn next(&mut self) -> Duration {
        let delay = Self::FIRST
            .checked_mul(1u32.checked_shl(self.failures).unwrap_or(u32::MAX))
            .map_or(Self::LIMIT, |d| d.min(Self::LIMIT));
        self.failures = self.failures.saturating_add(1);
        delay
    }
    pub(super) fn reset(&mut self) {
        self.failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATES: [Connection; 5] = [
        Connection::Connecting,
        Connection::Synchronizing,
        Connection::Connected,
        Connection::Waiting,
        Connection::Stopped,
    ];
    const SIGNALS: [Signal; 5] = [
        Signal::Greeted,
        Signal::Synchronized,
        Signal::Failed,
        Signal::RetryDue,
        Signal::Stop,
    ];

    #[test]
    fn transition_table_is_exact() {
        use Connection::*;
        let expected = |from: Connection, signal: Signal| match (from, signal) {
            (Connecting, Signal::Greeted) => Some(Synchronizing),
            (Connecting, Signal::Failed) => Some(Waiting),
            (Synchronizing, Signal::Synchronized) => Some(Connected),
            (Synchronizing, Signal::Failed) => Some(Waiting),
            (Connected, Signal::Synchronized) => Some(Connected),
            (Connected, Signal::Failed) => Some(Waiting),
            (Waiting, Signal::RetryDue) => Some(Connecting),
            (Connecting | Synchronizing | Connected | Waiting, Signal::Stop) => Some(Stopped),
            _ => None,
        };
        for from in STATES {
            for signal in SIGNALS {
                assert_eq!(
                    from.on(signal),
                    expected(from, signal),
                    "{from:?} {signal:?}"
                );
            }
        }
    }

    #[test]
    fn stopped_is_terminal_and_reachable_from_every_other_state() {
        for from in STATES {
            if from == Connection::Stopped {
                assert!(SIGNALS.iter().all(|s| from.on(*s).is_none()));
            } else {
                assert_eq!(from.on(Signal::Stop), Some(Connection::Stopped));
            }
        }
    }

    #[test]
    fn backoff_doubles_to_a_limit_and_resets() {
        let mut backoff = Backoff::default();
        let delays: Vec<u64> = (0..8).map(|_| backoff.next().as_millis() as u64).collect();
        assert_eq!(delays, [250, 500, 1000, 2000, 4000, 5000, 5000, 5000]);
        // No count of failures overflows the delay.
        backoff.failures = u32::MAX;
        assert_eq!(backoff.next(), Backoff::LIMIT);
        backoff.reset();
        assert_eq!(backoff.next(), Backoff::FIRST);
    }

    #[test]
    fn faults_separate_absence_loss_incompatibility_and_malformed_peers() {
        use ControlError as E;
        assert_eq!(Fault::connecting(E::TransportError), Fault::Absent);
        assert_eq!(Fault::of(E::TransportError), Fault::Lost);
        assert_eq!(Fault::of(E::Timeout), Fault::Lost);
        assert_eq!(Fault::of(E::TruncatedFrame), Fault::Lost);
        for e in [E::ProtocolVersionUnsupported, E::StorageIncompatible] {
            assert_eq!(Fault::of(e), Fault::Incompatible);
            assert_eq!(Fault::connecting(e), Fault::Incompatible);
        }
        for e in [E::UnsafePath, E::PeerRejected] {
            assert_eq!(Fault::connecting(e), Fault::UnsafePath);
        }
        for e in [
            E::InvalidRequest,
            E::CorrelationError,
            E::FrameTooLarge,
            E::HandshakeRequired,
            E::InternalError,
        ] {
            assert_eq!(Fault::of(e), Fault::Malformed);
            assert_eq!(Fault::connecting(e), Fault::Malformed);
        }
    }
}
