//! Transport-neutral, versioned remote envelopes and terminal stream contracts.
//!
//! These closed contracts do not open a listener, authenticate an endpoint, or transport bytes.
//! The selected network adapter must establish those properties before it can call the gateway.
use super::{device::DeviceId, grants::Grant};
use serde::{Deserialize, Serialize};

pub const REMOTE_PROTOCOL_VERSION: u8 = 1;
pub const MAX_REMOTE_INPUT_BYTES: usize = 4096;
pub const MAX_TERMINAL_FRAME_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    UnsupportedVersion,
    InvalidIdentity,
    InvalidGeneration,
    InvalidRequest,
    InvalidInput,
    InvalidAcknowledgement,
    InvalidCheckpoint,
    InvalidFrame,
}

/// Header fields shared by the future host and PC/Android transports. Payloads and host paths are
/// deliberately absent; session and stream identities are opaque bounded tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteEnvelopeV1 {
    pub protocol_version: u8,
    pub host_id: String,
    pub device_id: String,
    pub grant: Grant,
    pub connection_epoch: u64,
    pub request_id: String,
    pub session_id: Option<String>,
    pub session_generation: Option<u64>,
    pub stream_id: Option<String>,
}

impl RemoteEnvelopeV1 {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.protocol_version != REMOTE_PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion);
        }
        if !opaque_id(&self.host_id, 64)
            || DeviceId::parse(&self.device_id).is_none()
            || self.connection_epoch == 0
            || !canonical_uuid(&self.request_id)
        {
            return Err(ProtocolError::InvalidIdentity);
        }
        match (&self.session_id, self.session_generation) {
            (None, None) if self.stream_id.is_none() => Ok(()),
            (Some(session), Some(generation))
                if opaque_id(session, 64)
                    && generation > 0
                    && self
                        .stream_id
                        .as_deref()
                        .is_none_or(|stream| opaque_id(stream, 64)) =>
            {
                Ok(())
            }
            _ => Err(ProtocolError::InvalidGeneration),
        }
    }
}

/// Volatile input bytes for one authenticated claim. Serializing it never implies idempotency.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalInputV1 {
    pub session_generation: u64,
    pub connection_epoch: u64,
    pub claim_epoch: u64,
    pub sequence: u64,
    pub bytes: Vec<u8>,
}

impl TerminalInputV1 {
    pub fn validate(&self, envelope: &RemoteEnvelopeV1) -> Result<(), ProtocolError> {
        envelope.validate()?;
        if envelope.session_generation != Some(self.session_generation)
            || self.connection_epoch != envelope.connection_epoch
            || self.claim_epoch == 0
            || self.sequence == 0
            || self.bytes.is_empty()
            || self.bytes.len() > MAX_REMOTE_INPUT_BYTES
        {
            return Err(ProtocolError::InvalidInput);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputDeliveryV1 {
    Accepted,
    Partial,
    Rejected,
    OutcomeUnknown,
}

/// An ACK that reports a precise count; unknown delivery carries no invented accepted count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalInputAckV1 {
    pub request_id: String,
    pub session_generation: u64,
    pub connection_epoch: u64,
    pub claim_epoch: u64,
    pub sequence: u64,
    pub requested_bytes: u16,
    pub accepted_bytes: Option<u16>,
    pub delivery: InputDeliveryV1,
}

impl TerminalInputAckV1 {
    pub fn validate_for(
        &self,
        envelope: &RemoteEnvelopeV1,
        input: &TerminalInputV1,
    ) -> Result<(), ProtocolError> {
        input
            .validate(envelope)
            .map_err(|_| ProtocolError::InvalidAcknowledgement)?;
        let Some(accepted) = self.accepted_bytes else {
            return if self.matches(envelope, input)
                && self.delivery == InputDeliveryV1::OutcomeUnknown
                && usize::from(self.requested_bytes) == input.bytes.len()
            {
                Ok(())
            } else {
                Err(ProtocolError::InvalidAcknowledgement)
            };
        };
        if !self.matches(envelope, input)
            || usize::from(self.requested_bytes) != input.bytes.len()
            || accepted > self.requested_bytes
        {
            return Err(ProtocolError::InvalidAcknowledgement);
        }
        let valid_count = match self.delivery {
            InputDeliveryV1::Accepted => accepted == self.requested_bytes,
            InputDeliveryV1::Partial => accepted > 0 && accepted < self.requested_bytes,
            InputDeliveryV1::Rejected => accepted == 0,
            InputDeliveryV1::OutcomeUnknown => false,
        };
        valid_count
            .then_some(())
            .ok_or(ProtocolError::InvalidAcknowledgement)
    }

    fn matches(&self, envelope: &RemoteEnvelopeV1, input: &TerminalInputV1) -> bool {
        self.request_id == envelope.request_id
            && self.session_generation == input.session_generation
            && self.connection_epoch == input.connection_epoch
            && self.connection_epoch == envelope.connection_epoch
            && self.claim_epoch == input.claim_epoch
            && self.sequence == input.sequence
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalCheckpointV1 {
    pub session_generation: u64,
    pub connection_epoch: u64,
    pub frame_offset: u64,
    pub screen_digest: String,
}

impl TerminalCheckpointV1 {
    pub fn validate(&self, envelope: &RemoteEnvelopeV1) -> Result<(), ProtocolError> {
        envelope.validate()?;
        if envelope.session_generation != Some(self.session_generation)
            || envelope.connection_epoch != self.connection_epoch
            || !digest(&self.screen_digest)
        {
            return Err(ProtocolError::InvalidCheckpoint);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TerminalFrameV1 {
    pub session_generation: u64,
    pub connection_epoch: u64,
    pub offset: u64,
    pub bytes: Vec<u8>,
}

impl TerminalFrameV1 {
    pub fn validate(&self, envelope: &RemoteEnvelopeV1) -> Result<(), ProtocolError> {
        envelope.validate()?;
        if envelope.session_generation != Some(self.session_generation)
            || envelope.connection_epoch != self.connection_epoch
            || self.bytes.is_empty()
            || self.bytes.len() > MAX_TERMINAL_FRAME_BYTES
        {
            return Err(ProtocolError::InvalidFrame);
        }
        Ok(())
    }
}

fn opaque_id(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.hyphenated().to_string() == value)
}

fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope() -> RemoteEnvelopeV1 {
        RemoteEnvelopeV1 {
            protocol_version: 1,
            host_id: "host_A".into(),
            device_id: "a".repeat(32),
            grant: Grant::TerminalWrite,
            connection_epoch: 3,
            request_id: "00000000-0000-4000-8000-000000000001".into(),
            session_id: Some("session_A".into()),
            session_generation: Some(7),
            stream_id: Some("stream_A".into()),
        }
    }

    fn input(bytes: Vec<u8>) -> TerminalInputV1 {
        TerminalInputV1 {
            session_generation: 7,
            connection_epoch: 3,
            claim_epoch: 5,
            sequence: 11,
            bytes,
        }
    }

    #[test]
    fn envelope_is_versioned_closed_and_binds_all_remote_generations() {
        let mut header = envelope();
        assert_eq!(header.validate(), Ok(()));
        header.connection_epoch = 0;
        assert_eq!(header.validate(), Err(ProtocolError::InvalidIdentity));
        let mut header = envelope();
        header.protocol_version = 2;
        assert_eq!(header.validate(), Err(ProtocolError::UnsupportedVersion));
        let mut header = envelope();
        header.session_generation = None;
        assert_eq!(header.validate(), Err(ProtocolError::InvalidGeneration));
        assert!(serde_json::from_str::<RemoteEnvelopeV1>(
            r#"{"protocol_version":1,"host_id":"h","device_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","grant":"terminal_write","connection_epoch":3,"request_id":"00000000-0000-4000-8000-000000000001","session_id":null,"session_generation":null,"stream_id":null,"extra":true}"#
        )
        .is_err());
    }

    #[test]
    fn bounded_input_rejects_stale_session_or_connection_epochs() {
        let header = envelope();
        assert_eq!(
            input(vec![0; MAX_REMOTE_INPUT_BYTES]).validate(&header),
            Ok(())
        );
        assert_eq!(
            input(vec![0; MAX_REMOTE_INPUT_BYTES + 1]).validate(&header),
            Err(ProtocolError::InvalidInput)
        );
        let mut stale = input(vec![1]);
        stale.session_generation += 1;
        assert_eq!(stale.validate(&header), Err(ProtocolError::InvalidInput));
        let mut stale = input(vec![1]);
        stale.connection_epoch += 1;
        assert_eq!(stale.validate(&header), Err(ProtocolError::InvalidInput));
    }

    #[test]
    fn input_ack_reports_exact_partial_and_unknown_delivery_without_invented_counts() {
        let header = envelope();
        let request = input(vec![1, 2, 3, 4]);
        let ack = |delivery, accepted_bytes| TerminalInputAckV1 {
            request_id: header.request_id.clone(),
            session_generation: request.session_generation,
            connection_epoch: request.connection_epoch,
            claim_epoch: request.claim_epoch,
            sequence: request.sequence,
            requested_bytes: request.bytes.len() as u16,
            accepted_bytes,
            delivery,
        };
        assert_eq!(
            ack(InputDeliveryV1::Partial, Some(2)).validate_for(&header, &request),
            Ok(())
        );
        assert_eq!(
            ack(InputDeliveryV1::Accepted, Some(3)).validate_for(&header, &request),
            Err(ProtocolError::InvalidAcknowledgement)
        );
        assert_eq!(
            ack(InputDeliveryV1::OutcomeUnknown, None).validate_for(&header, &request),
            Ok(())
        );
        assert_eq!(
            ack(InputDeliveryV1::OutcomeUnknown, Some(0)).validate_for(&header, &request),
            Err(ProtocolError::InvalidAcknowledgement)
        );
    }

    #[test]
    fn frames_and_checkpoints_cannot_cross_generation_or_exceed_bounds() {
        let header = envelope();
        let frame = TerminalFrameV1 {
            session_generation: 7,
            connection_epoch: 3,
            offset: 80,
            bytes: vec![1; MAX_TERMINAL_FRAME_BYTES],
        };
        assert_eq!(frame.validate(&header), Ok(()));
        let mut oversized = frame.clone();
        oversized.bytes.push(1);
        assert_eq!(
            oversized.validate(&header),
            Err(ProtocolError::InvalidFrame)
        );
        let checkpoint = TerminalCheckpointV1 {
            session_generation: 7,
            connection_epoch: 3,
            frame_offset: 80,
            screen_digest: "a".repeat(64),
        };
        assert_eq!(checkpoint.validate(&header), Ok(()));
        let mut stale = checkpoint;
        stale.session_generation += 1;
        assert_eq!(
            stale.validate(&header),
            Err(ProtocolError::InvalidCheckpoint)
        );
    }
}
