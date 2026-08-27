//! Selected-profile absolute teleport projection and acknowledgement state for R2B 26.2.
//!
//! The fresh route always uses an absolute `PositionMoveRotation`: position `(x,y,z)`, zero delta
//! movement, yaw/pitch, and an empty `Relative` set. The reviewed empty-set pack law is therefore a
//! fixed network-order `i32(0)` mask; no `Set`, `EnumSet`, or per-relative lookup exists on Crucible's
//! join hot path.
//!
//! Teleport acknowledgement is connection state rather than publication state. The transaction
//! retains the source sequence law and does not clear pending state for wrong/stale acknowledgements.
//! Serverbound packet identity is a target-owned static fact from the reviewed 26.2 `GameProtocols`
//! insertion order; callers consume the semantic decoder and never perform a runtime packet lookup.

use crucible_connection_core::FrameView;
use crucible_packet_core::{PacketCodecError, PacketReader, PacketWriter};

const ABSOLUTE_FIXED_BYTES: usize = 24 + 24 + 8 + 4;
// VAR-NET-R2B-PLAY-GAME-PROTOCOLS-001: protocol 776 serverbound teleport confirmation.
const SERVERBOUND_TELEPORT_ACK_PACKET_ID: i32 = 0;

/// Decodes the selected 26.2 serverbound teleport acknowledgement surface.
///
/// Returns `Ok(None)` for every other Play packet so continuing Play composition can directly route
/// independent semantic slices without a packet registry. A matching packet contains exactly one
/// signed Minecraft `VarInt` teleport sequence ID.
///
/// # Errors
///
/// Returns the packet codec error for a truncated/overlong acknowledgement ID or trailing payload
/// bytes. The borrowed frame is never mutated by decoding.
pub fn decode_serverbound_teleport_ack(
    frame: FrameView<'_>,
) -> Result<Option<i32>, PacketCodecError> {
    if frame.packet_id() != SERVERBOUND_TELEPORT_ACK_PACKET_ID {
        return Ok(None);
    }
    let mut reader = PacketReader::new(frame.payload());
    let id = reader.read_var_int()?;
    reader.finish()?;
    Ok(Some(id))
}

/// One absolute fresh-player teleport payload without packet identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AbsoluteTeleportPayload {
    /// Source sequence/acknowledgement ID.
    pub id: i32,
    /// Absolute X coordinate.
    pub x: f64,
    /// Absolute Y coordinate.
    pub y: f64,
    /// Absolute Z coordinate.
    pub z: f64,
    /// Absolute yaw.
    pub yaw: f32,
    /// Absolute pitch.
    pub pitch: f32,
}

impl AbsoluteTeleportPayload {
    /// Encodes `id + position Vec3 + Vec3.ZERO + yaw + pitch + relativeMask(0)`.
    ///
    /// # Errors
    ///
    /// Performs one complete payload preflight before mutation, preserving any existing packet-ID
    /// prefix when the bounded writer cannot hold the entire teleport body.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        let payload_len = var_int_len(self.id)
            .checked_add(ABSOLUTE_FIXED_BYTES)
            .ok_or(PacketCodecError::LengthOverflow)?;
        preflight(writer, payload_len)?;

        writer.write_var_int(self.id)?;
        writer.write_f64(self.x)?;
        writer.write_f64(self.y)?;
        writer.write_f64(self.z)?;
        writer.write_f64(0.0)?;
        writer.write_f64(0.0)?;
        writer.write_f64(0.0)?;
        writer.write_f32(self.yaw)?;
        writer.write_f32(self.pitch)?;
        writer.write_i32(0)
    }
}

/// Position retained while the joining client owes an exact teleport acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AwaitingTeleport {
    /// Expected acknowledgement ID.
    pub id: i32,
    /// Server semantic X position installed before publication.
    pub x: f64,
    /// Server semantic Y position installed before publication.
    pub y: f64,
    /// Server semantic Z position installed before publication.
    pub z: f64,
}

/// Result of consuming one decoded teleport acknowledgement ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TeleportAckResult {
    /// The ID matched the outstanding transaction and pending state was cleared.
    Accepted,
    /// No teleport transaction was outstanding; duplicate/unsolicited acknowledgement.
    NoPendingTeleport,
    /// A different ID was received; the valid outstanding transaction remains pending.
    IdMismatch {
        /// Outstanding expected ID.
        expected: i32,
        /// Received stale/wrong ID.
        received: i32,
    },
}

/// Minimal connection-owned teleport sequence and pending-ack state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TeleportTransaction {
    last_id: i32,
    awaiting: Option<AwaitingTeleport>,
}

impl TeleportTransaction {
    /// Starts from vanilla's fresh-connection sequence state; the first issued ID is `1`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Issues/replaces the outstanding absolute teleport transaction.
    ///
    /// Vanilla increments first and maps `Integer.MAX_VALUE` to zero, so `i32::MAX` itself is never
    /// emitted by this sequence. A later teleport supersedes the earlier pending acknowledgement.
    pub fn issue(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
    ) -> AbsoluteTeleportPayload {
        self.last_id = if self.last_id == i32::MAX - 1 {
            0
        } else {
            self.last_id + 1
        };
        self.awaiting = Some(AwaitingTeleport {
            id: self.last_id,
            x,
            y,
            z,
        });
        AbsoluteTeleportPayload {
            id: self.last_id,
            x,
            y,
            z,
            yaw,
            pitch,
        }
    }

    /// Returns the currently outstanding acknowledgement state, if any.
    #[must_use]
    pub const fn awaiting(&self) -> Option<AwaitingTeleport> {
        self.awaiting
    }

    /// Applies one already-decoded client acknowledgement ID.
    ///
    /// Wrong/stale IDs are observational only and cannot destroy the live transaction. A matching
    /// acknowledgement clears it exactly once, making duplicates explicit `NoPendingTeleport`.
    pub fn acknowledge(&mut self, received: i32) -> TeleportAckResult {
        let Some(awaiting) = self.awaiting else {
            return TeleportAckResult::NoPendingTeleport;
        };
        if received != awaiting.id {
            return TeleportAckResult::IdMismatch {
                expected: awaiting.id,
                received,
            };
        }
        self.awaiting = None;
        TeleportAckResult::Accepted
    }
}

fn preflight(writer: &PacketWriter, additional: usize) -> Result<(), PacketCodecError> {
    if additional <= writer.remaining_capacity() {
        return Ok(());
    }
    let attempted = writer
        .len()
        .checked_add(additional)
        .ok_or(PacketCodecError::LengthOverflow)?;
    let maximum = writer
        .len()
        .checked_add(writer.remaining_capacity())
        .ok_or(PacketCodecError::LengthOverflow)?;
    Err(PacketCodecError::PacketLimitExceeded { attempted, maximum })
}

const fn var_int_len(value: i32) -> usize {
    let mut remaining = value.cast_unsigned();
    let mut length = 1_usize;
    while remaining & !0x7f != 0 {
        remaining >>= 7;
        length += 1;
    }
    length
}

#[cfg(test)]
mod tests {
    use crucible_connection_core::{ConnectionLimits, IngressBuffer};
    use crucible_packet_core::{PacketCodecError, PacketWriter};

    use super::{
        AbsoluteTeleportPayload, TeleportAckResult, TeleportTransaction,
        decode_serverbound_teleport_ack,
    };

    const CAPTURED: AbsoluteTeleportPayload = AbsoluteTeleportPayload {
        id: 1,
        x: 10.390_952_126_751_907,
        y: 84.0,
        z: -5.815_159_807_324_014,
        yaw: f32::from_bits(0xc2ee_3335),
        pitch: f32::from_bits(0x4190_0001),
    };

    fn ingress() -> IngressBuffer {
        IngressBuffer::new(ConnectionLimits::new(64, 128, 128).expect("test limits"))
    }

    #[test]
    fn serverbound_teleport_ack_decodes_source_admitted_identity() {
        let mut ingress = ingress();
        ingress
            .push(&[0x02, 0x00, 0x01])
            .expect("teleport ack frame fits");
        let frame = ingress
            .peek_frame()
            .expect("frame decode")
            .expect("one complete frame");
        assert_eq!(decode_serverbound_teleport_ack(frame), Ok(Some(1)));
    }

    #[test]
    fn teleport_ack_decoder_ignores_other_play_packets() {
        let mut ingress = ingress();
        ingress
            .push(&[0x02, 0x1c, 0x01])
            .expect("different packet frame fits");
        let frame = ingress
            .peek_frame()
            .expect("frame decode")
            .expect("one complete frame");
        assert_eq!(decode_serverbound_teleport_ack(frame), Ok(None));
    }

    #[test]
    fn teleport_ack_requires_exact_payload_consumption() {
        let mut ingress = ingress();
        ingress
            .push(&[0x03, 0x00, 0x01, 0x00])
            .expect("trailing-byte frame fits");
        let frame = ingress
            .peek_frame()
            .expect("frame decode")
            .expect("one complete frame");
        assert_eq!(
            decode_serverbound_teleport_ack(frame),
            Err(PacketCodecError::TrailingBytes { remaining: 1 })
        );
    }

    #[test]
    fn first_selected_teleport_matches_exact_r1x_golden_payload() {
        let expected: &[u8] = &[
            0x01, 0x40, 0x24, 0xc8, 0x2a, 0xe0, 0x8d, 0x66, 0xf5, 0x40, 0x55, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0xc0, 0x17, 0x42, 0xb9, 0x40, 0xa5, 0xe1, 0x97, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc2, 0xee, 0x33, 0x35, 0x41, 0x90, 0x00,
            0x01, 0x00, 0x00, 0x00, 0x00,
        ];
        let mut writer = PacketWriter::new(expected.len()).expect("exact teleport payload bound");
        CAPTURED
            .encode(&mut writer)
            .expect("captured teleport fits");
        assert_eq!(writer.as_slice(), expected);
    }

    #[test]
    fn whole_payload_rejection_preserves_packet_id_prefix() {
        let mut writer = PacketWriter::new(61).expect("one byte short after packet id");
        writer.write_u8(0x48).expect("position packet id");
        assert_eq!(
            CAPTURED.encode(&mut writer),
            Err(PacketCodecError::PacketLimitExceeded {
                attempted: 62,
                maximum: 61,
            })
        );
        assert_eq!(writer.as_slice(), &[0x48]);
    }

    #[test]
    fn first_issue_is_one_and_matching_ack_clears_exactly_once() {
        let mut state = TeleportTransaction::new();
        let payload = state.issue(1.0, 2.0, 3.0, 4.0, 5.0);
        assert_eq!(payload.id, 1);
        assert_eq!(state.awaiting().expect("pending teleport").id, 1);
        assert_eq!(state.acknowledge(1), TeleportAckResult::Accepted);
        assert_eq!(state.awaiting(), None);
        assert_eq!(state.acknowledge(1), TeleportAckResult::NoPendingTeleport);
    }

    #[test]
    fn wrong_or_stale_ack_never_destroys_current_transaction() {
        let mut state = TeleportTransaction::new();
        let first = state.issue(1.0, 2.0, 3.0, 0.0, 0.0);
        let second = state.issue(4.0, 5.0, 6.0, 0.0, 0.0);
        assert_eq!(first.id, 1);
        assert_eq!(second.id, 2);
        assert_eq!(
            state.acknowledge(first.id),
            TeleportAckResult::IdMismatch {
                expected: second.id,
                received: first.id,
            }
        );
        assert_eq!(
            state.awaiting().expect("second remains pending").id,
            second.id
        );
        assert_eq!(
            state.acknowledge(99),
            TeleportAckResult::IdMismatch {
                expected: 2,
                received: 99
            }
        );
        assert_eq!(state.awaiting().expect("still pending").id, 2);
        assert_eq!(state.acknowledge(2), TeleportAckResult::Accepted);
    }

    #[test]
    fn sequence_wraps_before_integer_max_value() {
        let mut state = TeleportTransaction {
            last_id: i32::MAX - 2,
            awaiting: None,
        };
        assert_eq!(state.issue(0.0, 0.0, 0.0, 0.0, 0.0).id, i32::MAX - 1);
        assert_eq!(state.issue(0.0, 0.0, 0.0, 0.0, 0.0).id, 0);
        assert_eq!(state.issue(0.0, 0.0, 0.0, 0.0, 0.0).id, 1);
    }
}
