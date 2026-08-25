//! Source-admitted Minecraft Java 26.2 Play-liveness wire binding.
//!
//! The reusable liveness state machine lives in `crucible-session-core`. This module owns only the
//! 26.2-specific timing policy and finite Play packet wire law. Packet identities are generated from
//! the admitted protocol contract; no runtime registry or target lookup is required.

use core::mem::size_of;

use crucible_connection_core::FrameView;
use crucible_session_core::LivenessPolicy;

mod generated {
    include!("generated/play_liveness_26_2.rs");
}

/// Exact Minecraft 26.2 keep-alive interval for the ordinary dedicated-server route.
pub const PLAY_KEEP_ALIVE_INTERVAL_MS: u64 = 15_000;
/// Exact Minecraft 26.2 closed-listener linger timeout.
pub const PLAY_CLOSED_LISTENER_TIMEOUT_MS: u64 = 15_000;
/// Exact packet-body size for one-byte Play packet identity plus signed 64-bit challenge.
pub const PLAY_KEEP_ALIVE_BODY_BYTES: usize = 1 + size_of::<i64>();
/// Source-admitted 26.2 timing policy supplied to the target-neutral liveness state machine.
pub const PLAY_LIVENESS_POLICY: LivenessPolicy =
    match LivenessPolicy::new(PLAY_KEEP_ALIVE_INTERVAL_MS, PLAY_CLOSED_LISTENER_TIMEOUT_MS) {
        Ok(policy) => policy,
        Err(_) => panic!("Minecraft 26.2 liveness policy must be positive and representable"),
    };

/// Fail-closed 26.2 Play-liveness codec error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayLivenessCodecError {
    /// The keep-alive packet carried anything other than the exact eight-byte signed challenge.
    InvalidPayloadLength {
        /// Actual payload bytes after the packet identity.
        actual: usize,
    },
}

/// Encodes one clientbound 26.2 Play keep-alive body without allocation.
#[must_use]
pub fn encode_clientbound_keep_alive(id: i64) -> [u8; PLAY_KEEP_ALIVE_BODY_BYTES] {
    let mut body = [0_u8; PLAY_KEEP_ALIVE_BODY_BYTES];
    body[0] = u8::try_from(generated::play::clientbound::KEEP_ALIVE)
        .expect("source-admitted clientbound keep-alive id fits one-byte VarInt");
    body[1..].copy_from_slice(&id.to_be_bytes());
    body
}

/// Decodes the finite serverbound 26.2 Play keep-alive surface.
///
/// Returns `Ok(None)` for every other Play packet so product composition may route those through
/// later semantic slices without this codec becoming a general Play registry.
///
/// # Errors
///
/// Returns [`PlayLivenessCodecError::InvalidPayloadLength`] when the source-admitted keep-alive
/// identity is present but its payload is not exactly one signed 64-bit value.
pub fn decode_serverbound_keep_alive(
    frame: FrameView<'_>,
) -> Result<Option<i64>, PlayLivenessCodecError> {
    if frame.packet_id() != generated::play::serverbound::KEEP_ALIVE {
        return Ok(None);
    }
    let payload = <&[u8; size_of::<i64>()]>::try_from(frame.payload()).map_err(|_| {
        PlayLivenessCodecError::InvalidPayloadLength {
            actual: frame.payload().len(),
        }
    })?;
    Ok(Some(i64::from_be_bytes(*payload)))
}

const _: () = assert!(PLAY_KEEP_ALIVE_BODY_BYTES == 9);

#[cfg(test)]
mod tests {
    use crucible_connection_core::{ConnectionLimits, FrameDecoder};

    use super::{
        PLAY_KEEP_ALIVE_BODY_BYTES, PLAY_LIVENESS_POLICY, PlayLivenessCodecError,
        decode_serverbound_keep_alive, encode_clientbound_keep_alive, generated,
    };

    #[test]
    fn generated_golden_clientbound_body_matches_stack_encoder() {
        let id = 0x0102_0304_0506_0708_i64;
        assert_eq!(
            encode_clientbound_keep_alive(id).as_slice(),
            generated::golden::PLAY_CLIENTBOUND_KEEP_ALIVE_BODY
        );
        assert_eq!(PLAY_KEEP_ALIVE_BODY_BYTES, 9);
    }

    #[test]
    fn generated_golden_serverbound_frame_decodes_exactly() {
        let limits = ConnectionLimits::new(64, 128, 128).expect("test limits");
        let mut decoder = FrameDecoder::new(limits);
        decoder
            .ingest::<()>(generated::golden::PLAY_SERVERBOUND_KEEP_ALIVE_FRAME)
            .expect("golden frame fits");
        let frame = decoder.next_frame().expect("decode succeeds").expect("one frame");
        assert_eq!(
            decode_serverbound_keep_alive(frame),
            Ok(Some(0x0102_0304_0506_0708_i64))
        );
    }

    #[test]
    fn malformed_keep_alive_payload_fails_closed() {
        let limits = ConnectionLimits::new(64, 128, 128).expect("test limits");
        let mut decoder = FrameDecoder::new(limits);
        let packet_id = u8::try_from(generated::play::serverbound::KEEP_ALIVE)
            .expect("source-admitted id fits one byte");
        decoder.ingest::<()>(&[0x01, packet_id]).expect("frame fits");
        let frame = decoder.next_frame().expect("decode succeeds").expect("one frame");
        assert_eq!(
            decode_serverbound_keep_alive(frame),
            Err(PlayLivenessCodecError::InvalidPayloadLength { actual: 0 })
        );
    }

    #[test]
    fn source_admitted_policy_is_exact() {
        assert_eq!(PLAY_LIVENESS_POLICY.keep_alive_interval_ms(), 15_000);
        assert_eq!(PLAY_LIVENESS_POLICY.closed_timeout_ms(), 15_000);
    }
}
