//! Source-admitted Minecraft Java 26.2 pre-play target for Crucible.
//!
//! This crate is the first target-version semantic layer above Crucible's target-neutral packet,
//! connection and pre-play machinery. It implements only the finite R0 surface admitted by
//! `PROTO-NET-STATUS-26-2-001`: Handshake -> Status -> Status request / response -> Ping / Pong.
//!
//! Packet identities are generated from the admitted contract. Runtime dispatch is direct static
//! matching; there is no packet registry, target lookup, trait object, socket runtime or second
//! framing/buffering layer here.

#![forbid(unsafe_code)]

use crucible_connection_core::FrameView;
use crucible_connection_driver::OutboundBatch;
use crucible_packet_core::{PacketCodecError, PacketReader, PacketWriter};
use crucible_preplay_core::{PrePlayAction, PrePlayTarget};
use crucible_session_core::{SessionPhase, SessionState, SessionStateError};

/// Generated compile-time packet identities and qualification-only golden bytes.
pub mod generated {
    include!("generated/status_26_2.rs");
}

/// Source-admitted Java UTF-16 unit bound for the handshake server-address field.
pub const MAX_SERVER_ADDRESS_UTF16_UNITS: usize = 255;
/// Source-admitted Java UTF-16 unit bound for the status-response JSON string.
pub const MAX_STATUS_JSON_UTF16_UNITS: usize = 32_767;
/// Maximum packet-body bytes needed by the finite R0 target.
///
/// A Rust string containing at most 32,767 Java UTF-16 units can occupy at most 98,301 UTF-8 bytes.
/// The status response then needs at most three bytes for that byte-length VarInt and one byte for
/// packet ID zero.
pub const MAX_R0_PACKET_BODY_BYTES: usize = 98_305;

const STATUS_INTENT: i32 = 1;

/// Per-connection 26.2 state that is finer than the generic [`SessionPhase`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Target26_2State {
    status_response_sent: bool,
}

impl Target26_2State {
    /// Whether the one allowed status response has already committed on this connection.
    #[must_use]
    pub const fn status_response_sent(&self) -> bool {
        self.status_response_sent
    }
}

/// Fail-closed semantic/codec error from the admitted 26.2 R0 target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target26_2Error {
    /// A packet ID is not admitted in the current target phase.
    UnknownPacket {
        /// Current generic session phase.
        phase: SessionPhase,
        /// Rejected wire packet ID.
        packet_id: i32,
    },
    /// R0 supports only the source-admitted STATUS handshake intent.
    UnsupportedIntent(i32),
    /// R0 was asked to decode a phase outside Handshake/Status.
    UnsupportedPhase(SessionPhase),
    /// A packet body violated the source-admitted field law.
    Codec(PacketCodecError),
    /// The generic lifecycle shell rejected an intended target transition.
    Transition(SessionStateError),
}

impl From<PacketCodecError> for Target26_2Error {
    fn from(value: PacketCodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<SessionStateError> for Target26_2Error {
    fn from(value: SessionStateError) -> Self {
        Self::Transition(value)
    }
}

/// One owned R0 action proposed by [`Target26_2`] before transactional admission.
#[derive(Debug)]
pub struct Target26_2Action {
    candidate: SessionState,
    frames: Vec<Vec<u8>>,
    next_state: Target26_2State,
}

impl Target26_2Action {
    fn new(
        candidate: SessionState,
        frames: Vec<Vec<u8>>,
        next_state: Target26_2State,
    ) -> Self {
        Self {
            candidate,
            frames,
            next_state,
        }
    }
}

impl OutboundBatch for Target26_2Action {
    type Body = Vec<u8>;

    fn outbound_frames(&self) -> &[Self::Body] {
        &self.frames
    }
}

impl PrePlayAction for Target26_2Action {
    fn candidate_session(&self) -> SessionState {
        self.candidate
    }
}

/// Static Minecraft Java 26.2 / protocol-776 target adapter for the finite R0 status slice.
#[derive(Debug)]
pub struct Target26_2;

impl PrePlayTarget for Target26_2 {
    type Error = Target26_2Error;
    /// Already-constructed `ServerStatus` JSON supplied by the product adapter.
    ///
    /// This target owns the source-admitted JSON string envelope and bound, not product status
    /// policy or a Mojang-shaped object graph.
    type Context = str;
    type State = Target26_2State;
    type Action = Target26_2Action;

    fn decode(
        context: &Self::Context,
        session: SessionState,
        target_state: &Self::State,
        frame: FrameView<'_>,
    ) -> Result<Self::Action, Self::Error> {
        match session.phase() {
            SessionPhase::Handshake => decode_handshake(session, *target_state, frame),
            SessionPhase::Status => decode_status(context, session, *target_state, frame),
            phase => Err(Target26_2Error::UnsupportedPhase(phase)),
        }
    }

    fn commit_target_state(state: &mut Self::State, action: Self::Action) {
        *state = action.next_state;
    }
}

fn decode_handshake(
    session: SessionState,
    target_state: Target26_2State,
    frame: FrameView<'_>,
) -> Result<Target26_2Action, Target26_2Error> {
    if frame.packet_id() != generated::handshake::serverbound::CLIENT_INTENTION {
        return Err(Target26_2Error::UnknownPacket {
            phase: SessionPhase::Handshake,
            packet_id: frame.packet_id(),
        });
    }

    let mut reader = PacketReader::new(frame.payload());
    let _protocol_version = reader.read_var_int()?;
    let _server_address = reader.read_string(MAX_SERVER_ADDRESS_UTF16_UNITS)?;
    let _server_port = reader.read_u16()?;
    let intent = reader.read_var_int()?;
    reader.finish()?;

    if intent != STATUS_INTENT {
        return Err(Target26_2Error::UnsupportedIntent(intent));
    }

    let mut candidate = session;
    candidate.advance(SessionPhase::Status)?;
    Ok(Target26_2Action::new(
        candidate,
        Vec::new(),
        target_state,
    ))
}

fn decode_status(
    status_json: &str,
    session: SessionState,
    target_state: Target26_2State,
    frame: FrameView<'_>,
) -> Result<Target26_2Action, Target26_2Error> {
    match frame.packet_id() {
        generated::status::serverbound::STATUS_REQUEST => {
            let reader = PacketReader::new(frame.payload());
            reader.finish()?;

            if target_state.status_response_sent {
                let mut candidate = session;
                let _changed = candidate.close();
                return Ok(Target26_2Action::new(
                    candidate,
                    Vec::new(),
                    target_state,
                ));
            }

            let response = encode_status_response(status_json)?;
            Ok(Target26_2Action::new(
                session,
                vec![response],
                Target26_2State {
                    status_response_sent: true,
                },
            ))
        }
        generated::status::serverbound::PING_REQUEST => {
            let mut reader = PacketReader::new(frame.payload());
            let payload = reader.read_i64()?;
            reader.finish()?;

            let pong = encode_pong(payload)?;
            let mut candidate = session;
            let _changed = candidate.close();
            Ok(Target26_2Action::new(
                candidate,
                vec![pong],
                target_state,
            ))
        }
        packet_id => Err(Target26_2Error::UnknownPacket {
            phase: SessionPhase::Status,
            packet_id,
        }),
    }
}

fn encode_status_response(status_json: &str) -> Result<Vec<u8>, Target26_2Error> {
    let mut writer = PacketWriter::new(MAX_R0_PACKET_BODY_BYTES)?;
    writer.write_var_int(generated::status::clientbound::STATUS_RESPONSE)?;
    writer.write_string(status_json, MAX_STATUS_JSON_UTF16_UNITS)?;
    Ok(writer.into_bytes())
}

fn encode_pong(payload: i64) -> Result<Vec<u8>, Target26_2Error> {
    let mut writer = PacketWriter::new(MAX_R0_PACKET_BODY_BYTES)?;
    writer.write_var_int(generated::status::clientbound::PONG_RESPONSE)?;
    writer.write_i64(payload)?;
    Ok(writer.into_bytes())
}

#[cfg(test)]
mod tests {
    use crucible_connection_core::ConnectionLimits;
    use crucible_connection_driver::ConnectionDriver;
    use crucible_packet_core::{PacketCodecError, PacketField, PacketWriter};
    use crucible_preplay_core::{PrePlayConnection, PrePlayError, PrePlayProcess};
    use crucible_session_core::SessionPhase;

    use super::{
        MAX_R0_PACKET_BODY_BYTES, MAX_SERVER_ADDRESS_UTF16_UNITS, Target26_2, Target26_2Error,
        generated,
    };

    const ORACLE_STATUS_JSON: &str = "{\"description\":\"Crucible R0 Oracle\",\"players\":{\"max\":20,\"online\":0},\"version\":{\"name\":\"26.2\",\"protocol\":776},\"enforcesSecureChat\":true}";

    fn limits() -> ConnectionLimits {
        ConnectionLimits::new(
            MAX_R0_PACKET_BODY_BYTES,
            MAX_R0_PACKET_BODY_BYTES * 2,
            MAX_R0_PACKET_BODY_BYTES * 2,
        )
        .expect("coherent R0 test limits")
    }

    fn tight_limits() -> ConnectionLimits {
        ConnectionLimits::new(32, 256, 33).expect("one 32-byte frame fits")
    }

    fn encoded_frame(body: &[u8], limits: ConnectionLimits) -> Vec<u8> {
        let mut driver = ConnectionDriver::new(limits);
        driver.queue_frame::<()>(body).expect("test frame fits");
        driver.pending_egress().to_vec()
    }

    fn body(
        packet_id: i32,
        encode: impl FnOnce(&mut PacketWriter) -> Result<(), PacketCodecError>,
    ) -> Vec<u8> {
        let mut writer = PacketWriter::new(MAX_R0_PACKET_BODY_BYTES).expect("positive packet bound");
        writer.write_var_int(packet_id).expect("packet id fits");
        encode(&mut writer).expect("test payload fits");
        writer.into_bytes()
    }

    fn handshake_body(protocol: i32, intent: i32) -> Vec<u8> {
        body(
            generated::handshake::serverbound::CLIENT_INTENTION,
            |writer| {
                writer.write_var_int(protocol)?;
                writer.write_string("127.0.0.1", MAX_SERVER_ADDRESS_UTF16_UNITS)?;
                writer.write_u16(25_566)?;
                writer.write_var_int(intent)
            },
        )
    }

    fn enter_status(connection: &mut PrePlayConnection<Target26_2>) {
        connection
            .ingest(generated::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_FRAME)
            .expect("golden handshake ingress");
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Handshake,
                to: SessionPhase::Status,
                outbound_frames: 0,
            })
        );
    }

    fn drain(connection: &mut PrePlayConnection<Target26_2>) {
        let queued = connection.queued_egress();
        connection.consume_written(queued).expect("drain exact egress");
    }

    #[test]
    fn generated_identity_is_the_admitted_r0_contract() {
        assert_eq!(generated::CONTRACT_ID, "PROTO-NET-STATUS-26-2-001");
        assert_eq!(generated::MINECRAFT_VERSION, "26.2");
        assert_eq!(generated::PROTOCOL_VERSION, 776);
        assert_eq!(generated::handshake::serverbound::CLIENT_INTENTION, 0);
        assert_eq!(generated::status::serverbound::STATUS_REQUEST, 0);
        assert_eq!(generated::status::serverbound::PING_REQUEST, 1);
        assert_eq!(generated::status::clientbound::STATUS_RESPONSE, 0);
        assert_eq!(generated::status::clientbound::PONG_RESPONSE, 1);
    }

    #[test]
    fn real_oracle_golden_status_and_ping_exchange_is_byte_exact() {
        let mut connection = PrePlayConnection::<Target26_2>::new(limits());
        enter_status(&mut connection);
        assert!(!connection.target_state().status_response_sent());

        connection
            .ingest(generated::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME)
            .expect("golden status request ingress");
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Status,
                to: SessionPhase::Status,
                outbound_frames: 1,
            })
        );
        assert!(connection.target_state().status_response_sent());
        assert_eq!(
            connection.pending_egress(),
            generated::golden::STATUS_CLIENTBOUND_STATUS_RESPONSE_FRAME
        );
        drain(&mut connection);

        connection
            .ingest(generated::golden::STATUS_SERVERBOUND_PING_REQUEST_FRAME)
            .expect("golden ping ingress");
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Status,
                to: SessionPhase::Closed,
                outbound_frames: 1,
            })
        );
        assert_eq!(
            connection.pending_egress(),
            generated::golden::STATUS_CLIENTBOUND_PONG_RESPONSE_FRAME
        );
    }

    #[test]
    fn status_handshake_does_not_require_client_protocol_to_equal_776() {
        let mut connection = PrePlayConnection::<Target26_2>::new(limits());
        let frame = encoded_frame(&handshake_body(775, 1), limits());
        connection.ingest(&frame).expect("handshake ingress");
        assert!(matches!(
            connection.process_one(ORACLE_STATUS_JSON),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Handshake,
                to: SessionPhase::Status,
                outbound_frames: 0,
            })
        ));
    }

    #[test]
    fn unsupported_handshake_intent_is_rejected_without_consuming_input() {
        let mut connection = PrePlayConnection::<Target26_2>::new(limits());
        let frame = encoded_frame(&handshake_body(776, 2), limits());
        connection.ingest(&frame).expect("handshake ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(Target26_2Error::UnsupportedIntent(2)))
        );
        assert_eq!(connection.phase(), SessionPhase::Handshake);
        assert_eq!(connection.buffered_ingress(), buffered);
    }

    #[test]
    fn duplicate_status_request_commits_terminal_close_without_second_response() {
        let mut connection = PrePlayConnection::<Target26_2>::new(limits());
        enter_status(&mut connection);
        connection
            .ingest(generated::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME)
            .expect("first status request");
        connection
            .process_one(ORACLE_STATUS_JSON)
            .expect("first status response commits");
        drain(&mut connection);

        connection
            .ingest(generated::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME)
            .expect("duplicate status request");
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Status,
                to: SessionPhase::Closed,
                outbound_frames: 0,
            })
        );
        assert_eq!(connection.queued_egress(), 0);
        assert_eq!(connection.buffered_ingress(), 0);
    }

    #[test]
    fn nonempty_status_request_is_rejected_transactionally() {
        let mut connection = PrePlayConnection::<Target26_2>::new(limits());
        enter_status(&mut connection);
        let malformed = body(generated::status::serverbound::STATUS_REQUEST, |writer| {
            writer.write_bool(true)
        });
        let frame = encoded_frame(&malformed, limits());
        connection.ingest(&frame).expect("malformed request ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(Target26_2Error::Codec(
                PacketCodecError::TrailingBytes { remaining: 1 }
            )))
        );
        assert!(!connection.target_state().status_response_sent());
        assert_eq!(connection.buffered_ingress(), buffered);
        assert_eq!(connection.queued_egress(), 0);
    }

    #[test]
    fn truncated_ping_is_rejected_without_consuming_input() {
        let mut connection = PrePlayConnection::<Target26_2>::new(limits());
        enter_status(&mut connection);
        let truncated = body(generated::status::serverbound::PING_REQUEST, |writer| {
            writer.write_bytes(&[0_u8; 7])
        });
        let frame = encoded_frame(&truncated, limits());
        connection.ingest(&frame).expect("truncated ping ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(Target26_2Error::Codec(
                PacketCodecError::Truncated {
                    field: PacketField::I64,
                    remaining: 7,
                }
            )))
        );
        assert_eq!(connection.phase(), SessionPhase::Status);
        assert_eq!(connection.buffered_ingress(), buffered);
    }

    #[test]
    fn signed_ping_payload_is_echoed_bit_exact_then_connection_closes() {
        let mut connection = PrePlayConnection::<Target26_2>::new(limits());
        enter_status(&mut connection);
        let ping = body(generated::status::serverbound::PING_REQUEST, |writer| {
            writer.write_i64(-0x0102_0304_0506_0708_i64)
        });
        let expected = body(generated::status::clientbound::PONG_RESPONSE, |writer| {
            writer.write_i64(-0x0102_0304_0506_0708_i64)
        });
        connection
            .ingest(&encoded_frame(&ping, limits()))
            .expect("signed ping ingress");
        assert!(matches!(
            connection.process_one(ORACLE_STATUS_JSON),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Status,
                to: SessionPhase::Closed,
                outbound_frames: 1,
            })
        ));
        assert_eq!(connection.pending_egress(), encoded_frame(&expected, limits()));
    }

    #[test]
    fn egress_rejection_rolls_back_status_one_shot_state_and_input() {
        let limits = tight_limits();
        let mut connection = PrePlayConnection::<Target26_2>::new(limits);
        connection
            .ingest(generated::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_FRAME)
            .expect("small-limit handshake ingress");
        connection
            .process_one(ORACLE_STATUS_JSON)
            .expect("small-limit handshake commits");

        connection
            .ingest(generated::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME)
            .expect("status request ingress");
        let buffered = connection.buffered_ingress();
        assert!(matches!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Buffer(_))
        ));
        assert_eq!(connection.phase(), SessionPhase::Status);
        assert!(!connection.target_state().status_response_sent());
        assert_eq!(connection.buffered_ingress(), buffered);
        assert_eq!(connection.queued_egress(), 0);
    }
}
