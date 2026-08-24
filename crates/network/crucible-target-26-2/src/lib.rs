//! Source-admitted Minecraft Java 26.2 pre-play target for Crucible.
//!
//! This crate is the target-version semantic layer above Crucible's target-neutral packet,
//! connection and pre-play machinery. It currently implements two independently admitted finite
//! surfaces:
//!
//! - R0: Handshake -> Status -> Status response -> Ping/Pong;
//! - R1A: Handshake(LOGIN) -> offline Hello -> LoginFinished -> LoginAcknowledged -> Configuration.
//!
//! Packet identities are generated from admitted contracts. Runtime dispatch is direct static
//! matching; there is no packet registry, target lookup, trait object, socket runtime or second
//! framing/buffering layer here.

#![forbid(unsafe_code)]

mod offline_uuid;

use crucible_connection_core::FrameView;
use crucible_connection_driver::OutboundBatch;
use crucible_packet_core::{PacketCodecError, PacketReader, PacketWriter};
use crucible_preplay_core::{PrePlayAction, PrePlayTarget};
use crucible_session_core::{SessionPhase, SessionState, SessionStateError};

/// Generated compile-time packet identities and qualification-only golden bytes.
///
/// The historical top-level exports remain the R0 Status contract so existing product startup and
/// qualification code stays source-compatible. The independently admitted Login contract lives in
/// [`generated::login_26_2`].
pub mod generated {
    include!("generated/status_26_2.rs");

    /// Generated packet identities for `PROTO-NET-LOGIN-26-2-001`.
    pub mod login_26_2 {
        include!("generated/login_26_2.rs");
    }
}

/// Source-admitted Java UTF-16 unit bound for the handshake server-address field.
pub const MAX_SERVER_ADDRESS_UTF16_UNITS: usize = 255;
/// Source-admitted Java UTF-16 unit bound for a Login player name.
pub const MAX_PLAYER_NAME_UTF16_UNITS: usize = 16;
/// Source-admitted Java UTF-16 unit bound for the status-response JSON string.
pub const MAX_STATUS_JSON_UTF16_UNITS: usize = 32_767;
/// Maximum packet-body bytes needed by the finite R0 target.
///
/// A Rust string containing at most 32,767 Java UTF-16 units can occupy at most 98,301 UTF-8 bytes.
/// The status response then needs at most three bytes for that byte-length `VarInt` and one byte for
/// packet ID zero.
pub const MAX_R0_PACKET_BODY_BYTES: usize = 98_305;
/// Exact maximum body size for the selected zero-property R1A `LoginFinished` representation.
///
/// `id(1) + profile UUID(16) + name length(1) + name UTF-8(48) + property count(1) + session UUID(16)`.
pub const MAX_R1A_LOGIN_PACKET_BODY_BYTES: usize = 83;

const STATUS_INTENT: i32 = 1;
const LOGIN_INTENT: i32 = 2;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum LoginStage {
    #[default]
    AwaitHello,
    AwaitAcknowledgement,
    Accepted,
}

/// Per-connection 26.2 state finer than the generic [`SessionPhase`].
///
/// The session UUID is runtime connection state rather than a version constant. Product code that
/// intends to admit Login must construct the target state with [`Self::with_login_session_uuid`].
/// Status-only connections can continue to use [`Default`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Target26_2State {
    status_response_sent: bool,
    login_stage: LoginStage,
    login_session_uuid: Option<[u8; 16]>,
}

impl Target26_2State {
    /// Creates target-local state capable of the admitted R1A Login route.
    #[must_use]
    pub const fn with_login_session_uuid(session_uuid: [u8; 16]) -> Self {
        Self {
            status_response_sent: false,
            login_stage: LoginStage::AwaitHello,
            login_session_uuid: Some(session_uuid),
        }
    }

    /// Whether the one allowed status response has already committed on this connection.
    #[must_use]
    pub const fn status_response_sent(&self) -> bool {
        self.status_response_sent
    }

    /// Whether `LoginFinished` has committed and the target is waiting for acknowledgement.
    #[must_use]
    pub const fn login_finished_sent(&self) -> bool {
        matches!(self.login_stage, LoginStage::AwaitAcknowledgement)
    }

    /// Whether the Login acknowledgement committed and Configuration entry is admitted.
    #[must_use]
    pub const fn login_accepted(&self) -> bool {
        matches!(self.login_stage, LoginStage::Accepted)
    }
}

/// Fail-closed semantic/codec error from the admitted 26.2 target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target26_2Error {
    /// A packet ID is not admitted in the current target phase.
    UnknownPacket {
        /// Current generic session phase.
        phase: SessionPhase,
        /// Rejected wire packet ID.
        packet_id: i32,
    },
    /// The handshake selected an intent outside the currently admitted Status/Login surfaces.
    UnsupportedIntent(i32),
    /// Login requires the exact pinned protocol version while Status deliberately remains tolerant.
    LoginProtocolMismatch {
        /// Pinned protocol required by Minecraft 26.2 Login.
        expected: i32,
        /// Protocol supplied by the peer.
        actual: i32,
    },
    /// Product code attempted Login without supplying the runtime server connection session UUID.
    MissingLoginSessionUuid,
    /// A source-known Login packet arrived in the wrong fine-grained Login state.
    UnexpectedLoginState,
    /// The requested Login name violated vanilla's `StringUtil.isValidPlayerName` law.
    InvalidPlayerName,
    /// The current target slice does not yet admit packets in this generic phase.
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

/// One owned target action proposed before transactional admission.
#[derive(Debug)]
pub struct Target26_2Action {
    candidate: SessionState,
    frames: Vec<Vec<u8>>,
    next_state: Target26_2State,
}

impl Target26_2Action {
    fn new(candidate: SessionState, frames: Vec<Vec<u8>>, next_state: Target26_2State) -> Self {
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

/// Static Minecraft Java 26.2 / protocol-776 target adapter.
#[derive(Debug)]
pub struct Target26_2;

impl PrePlayTarget for Target26_2 {
    type Error = Target26_2Error;
    /// Already-constructed `ServerStatus` JSON supplied by the product adapter.
    ///
    /// Login needs no product service object: its per-connection session UUID is target-local state.
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
            SessionPhase::Login => decode_login(session, *target_state, frame),
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
    let protocol_version = reader.read_var_int()?;
    let _server_address = reader.read_string(MAX_SERVER_ADDRESS_UTF16_UNITS)?;
    let _server_port = reader.read_u16()?;
    let intent = reader.read_var_int()?;
    reader.finish()?;

    let mut candidate = session;
    match intent {
        STATUS_INTENT => candidate.advance(SessionPhase::Status)?,
        LOGIN_INTENT => {
            let expected = generated::login_26_2::PROTOCOL_VERSION;
            if protocol_version != expected {
                return Err(Target26_2Error::LoginProtocolMismatch {
                    expected,
                    actual: protocol_version,
                });
            }
            if target_state.login_session_uuid.is_none() {
                return Err(Target26_2Error::MissingLoginSessionUuid);
            }
            candidate.advance(SessionPhase::Login)?;
        }
        unsupported => return Err(Target26_2Error::UnsupportedIntent(unsupported)),
    }

    Ok(Target26_2Action::new(candidate, Vec::new(), target_state))
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
                return Ok(Target26_2Action::new(candidate, Vec::new(), target_state));
            }

            let response = encode_status_response(status_json)?;
            let mut next_state = target_state;
            next_state.status_response_sent = true;
            Ok(Target26_2Action::new(session, vec![response], next_state))
        }
        generated::status::serverbound::PING_REQUEST => {
            let mut reader = PacketReader::new(frame.payload());
            let payload = reader.read_i64()?;
            reader.finish()?;

            let pong = encode_pong(payload)?;
            let mut candidate = session;
            let _changed = candidate.close();
            Ok(Target26_2Action::new(candidate, vec![pong], target_state))
        }
        packet_id => Err(Target26_2Error::UnknownPacket {
            phase: SessionPhase::Status,
            packet_id,
        }),
    }
}

fn decode_login(
    session: SessionState,
    target_state: Target26_2State,
    frame: FrameView<'_>,
) -> Result<Target26_2Action, Target26_2Error> {
    use generated::login_26_2::login::serverbound::{LOGIN_ACKNOWLEDGED, LOGIN_HELLO};

    match (target_state.login_stage, frame.packet_id()) {
        (LoginStage::AwaitHello, LOGIN_HELLO) => {
            let mut reader = PacketReader::new(frame.payload());
            let player_name = reader.read_string(MAX_PLAYER_NAME_UTF16_UNITS)?;
            let _client_uuid_msb = reader.read_u64()?;
            let _client_uuid_lsb = reader.read_u64()?;
            reader.finish()?;

            if !valid_player_name(player_name) {
                return Err(Target26_2Error::InvalidPlayerName);
            }

            let session_uuid = target_state
                .login_session_uuid
                .ok_or(Target26_2Error::MissingLoginSessionUuid)?;
            let profile_uuid = offline_uuid::offline_player_uuid(player_name);
            let response = encode_login_finished(player_name, profile_uuid, session_uuid)?;

            let mut next_state = target_state;
            next_state.login_stage = LoginStage::AwaitAcknowledgement;
            Ok(Target26_2Action::new(session, vec![response], next_state))
        }
        (LoginStage::AwaitAcknowledgement, LOGIN_ACKNOWLEDGED) => {
            let reader = PacketReader::new(frame.payload());
            reader.finish()?;

            let mut candidate = session;
            candidate.advance(SessionPhase::Configuration)?;
            let mut next_state = target_state;
            next_state.login_stage = LoginStage::Accepted;
            Ok(Target26_2Action::new(candidate, Vec::new(), next_state))
        }
        (_, LOGIN_HELLO | LOGIN_ACKNOWLEDGED) => Err(Target26_2Error::UnexpectedLoginState),
        (_, packet_id) => Err(Target26_2Error::UnknownPacket {
            phase: SessionPhase::Login,
            packet_id,
        }),
    }
}

fn valid_player_name(player_name: &str) -> bool {
    // Java 26.2: length <= 16 and no UTF-16 `char` <= 32 or >= 127. The codec has already enforced
    // the UTF-16 length, and the surviving character set is exactly printable ASCII 33..126.
    player_name
        .as_bytes()
        .iter()
        .all(|byte| (b'!'..=b'~').contains(byte))
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

fn encode_login_finished(
    player_name: &str,
    profile_uuid: [u8; 16],
    session_uuid: [u8; 16],
) -> Result<Vec<u8>, Target26_2Error> {
    let mut writer = PacketWriter::new(MAX_R1A_LOGIN_PACKET_BODY_BYTES)?;
    writer.write_var_int(generated::login_26_2::login::clientbound::LOGIN_FINISHED)?;
    writer.write_bytes(&profile_uuid)?;
    writer.write_string(player_name, MAX_PLAYER_NAME_UTF16_UNITS)?;
    writer.write_var_int(0)?;
    writer.write_bytes(&session_uuid)?;
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
        Target26_2State, generated,
    };

    const ORACLE_STATUS_JSON: &str = "{\"description\":\"Crucible R0 Oracle\",\"players\":{\"max\":20,\"online\":0},\"version\":{\"name\":\"26.2\",\"protocol\":776},\"enforcesSecureChat\":true}";
    const REAL_LOGIN_SESSION_UUID: [u8; 16] = [
        0x4d, 0x7f, 0x60, 0x4f, 0x19, 0x6a, 0x43, 0xb0, 0x89, 0x87, 0xf0, 0xb2, 0xa2, 0x7c, 0x26,
        0x63,
    ];

    fn limits() -> ConnectionLimits {
        ConnectionLimits::new(
            MAX_R0_PACKET_BODY_BYTES,
            MAX_R0_PACKET_BODY_BYTES * 2,
            MAX_R0_PACKET_BODY_BYTES * 2,
        )
        .expect("coherent target test limits")
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
        let mut writer =
            PacketWriter::new(MAX_R0_PACKET_BODY_BYTES).expect("positive packet bound");
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

    fn login_state() -> Target26_2State {
        Target26_2State::with_login_session_uuid(REAL_LOGIN_SESSION_UUID)
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

    fn enter_login(connection: &mut PrePlayConnection<Target26_2>) {
        connection
            .ingest(
                generated::login_26_2::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_LOGIN_FRAME,
            )
            .expect("golden Login handshake ingress");
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Handshake,
                to: SessionPhase::Login,
                outbound_frames: 0,
            })
        );
    }

    fn drain(connection: &mut PrePlayConnection<Target26_2>) {
        let queued = connection.queued_egress();
        connection
            .consume_written(queued)
            .expect("drain exact egress");
    }

    #[test]
    fn generated_identity_contains_both_admitted_contracts() {
        assert_eq!(generated::CONTRACT_ID, "PROTO-NET-STATUS-26-2-001");
        assert_eq!(generated::MINECRAFT_VERSION, "26.2");
        assert_eq!(generated::PROTOCOL_VERSION, 776);
        assert_eq!(generated::handshake::serverbound::CLIENT_INTENTION, 0);
        assert_eq!(generated::status::serverbound::STATUS_REQUEST, 0);
        assert_eq!(generated::status::serverbound::PING_REQUEST, 1);
        assert_eq!(generated::status::clientbound::STATUS_RESPONSE, 0);
        assert_eq!(generated::status::clientbound::PONG_RESPONSE, 1);

        assert_eq!(
            generated::login_26_2::CONTRACT_ID,
            "PROTO-NET-LOGIN-26-2-001"
        );
        assert_eq!(generated::login_26_2::PROTOCOL_VERSION, 776);
        assert_eq!(generated::login_26_2::login::serverbound::LOGIN_HELLO, 0);
        assert_eq!(
            generated::login_26_2::login::serverbound::LOGIN_ACKNOWLEDGED,
            3
        );
        assert_eq!(generated::login_26_2::login::clientbound::LOGIN_FINISHED, 2);
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
    fn real_client_login_exchange_is_byte_exact_through_configuration_entry() {
        let mut connection =
            PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
        enter_login(&mut connection);
        assert!(!connection.target_state().login_finished_sent());

        connection
            .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_HELLO_FRAME)
            .expect("real Login hello ingress");
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Login,
                to: SessionPhase::Login,
                outbound_frames: 1,
            })
        );
        assert!(connection.target_state().login_finished_sent());
        assert_eq!(
            connection.pending_egress(),
            generated::login_26_2::golden::LOGIN_CLIENTBOUND_LOGIN_FINISHED_FRAME
        );
        drain(&mut connection);

        connection
            .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_ACKNOWLEDGED_FRAME)
            .expect("real Login acknowledgement ingress");
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Ok(PrePlayProcess::Committed {
                from: SessionPhase::Login,
                to: SessionPhase::Configuration,
                outbound_frames: 0,
            })
        );
        assert!(connection.target_state().login_accepted());
        assert_eq!(connection.queued_egress(), 0);
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
    fn login_handshake_requires_exact_protocol_transactionally() {
        let mut connection =
            PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
        let frame = encoded_frame(&handshake_body(775, 2), limits());
        connection.ingest(&frame).expect("Login handshake ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(
                Target26_2Error::LoginProtocolMismatch {
                    expected: 776,
                    actual: 775,
                }
            ))
        );
        assert_eq!(connection.phase(), SessionPhase::Handshake);
        assert_eq!(connection.buffered_ingress(), buffered);
    }

    #[test]
    fn login_requires_runtime_session_uuid_without_consuming_handshake() {
        let mut connection = PrePlayConnection::<Target26_2>::new(limits());
        connection
            .ingest(
                generated::login_26_2::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_LOGIN_FRAME,
            )
            .expect("Login handshake ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(
                Target26_2Error::MissingLoginSessionUuid
            ))
        );
        assert_eq!(connection.phase(), SessionPhase::Handshake);
        assert_eq!(connection.buffered_ingress(), buffered);
    }

    #[test]
    fn unsupported_handshake_intent_is_rejected_without_consuming_input() {
        let mut connection = PrePlayConnection::<Target26_2>::new(limits());
        let frame = encoded_frame(&handshake_body(776, 99), limits());
        connection.ingest(&frame).expect("handshake ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(Target26_2Error::UnsupportedIntent(99)))
        );
        assert_eq!(connection.phase(), SessionPhase::Handshake);
        assert_eq!(connection.buffered_ingress(), buffered);
    }

    #[test]
    fn invalid_player_name_is_rejected_without_state_or_ingress_commit() {
        let mut connection =
            PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
        enter_login(&mut connection);
        let malformed = body(
            generated::login_26_2::login::serverbound::LOGIN_HELLO,
            |writer| {
                writer.write_string("bad name", 16)?;
                writer.write_u64(0)?;
                writer.write_u64(0)
            },
        );
        connection
            .ingest(&encoded_frame(&malformed, limits()))
            .expect("invalid-name hello ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(Target26_2Error::InvalidPlayerName))
        );
        assert_eq!(connection.phase(), SessionPhase::Login);
        assert!(!connection.target_state().login_finished_sent());
        assert_eq!(connection.buffered_ingress(), buffered);
        assert_eq!(connection.queued_egress(), 0);
    }

    #[test]
    fn acknowledgement_before_login_finished_is_rejected_transactionally() {
        let mut connection =
            PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
        enter_login(&mut connection);
        connection
            .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_ACKNOWLEDGED_FRAME)
            .expect("early acknowledgement ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(Target26_2Error::UnexpectedLoginState))
        );
        assert_eq!(connection.phase(), SessionPhase::Login);
        assert!(!connection.target_state().login_finished_sent());
        assert_eq!(connection.buffered_ingress(), buffered);
    }

    #[test]
    fn duplicate_hello_after_login_finished_is_rejected_transactionally() {
        let mut connection =
            PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
        enter_login(&mut connection);
        connection
            .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_HELLO_FRAME)
            .expect("first hello ingress");
        connection
            .process_one(ORACLE_STATUS_JSON)
            .expect("first hello commits");
        drain(&mut connection);

        connection
            .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_HELLO_FRAME)
            .expect("duplicate hello ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(Target26_2Error::UnexpectedLoginState))
        );
        assert!(connection.target_state().login_finished_sent());
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
        connection
            .ingest(&frame)
            .expect("malformed request ingress");
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
    fn nonempty_login_acknowledgement_is_rejected_transactionally() {
        let mut connection =
            PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
        enter_login(&mut connection);
        connection
            .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_HELLO_FRAME)
            .expect("hello ingress");
        connection
            .process_one(ORACLE_STATUS_JSON)
            .expect("hello commits");
        drain(&mut connection);

        let malformed = body(
            generated::login_26_2::login::serverbound::LOGIN_ACKNOWLEDGED,
            |writer| writer.write_bool(true),
        );
        connection
            .ingest(&encoded_frame(&malformed, limits()))
            .expect("nonempty acknowledgement ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(Target26_2Error::Codec(
                PacketCodecError::TrailingBytes { remaining: 1 }
            )))
        );
        assert_eq!(connection.phase(), SessionPhase::Login);
        assert!(connection.target_state().login_finished_sent());
        assert_eq!(connection.buffered_ingress(), buffered);
    }

    #[test]
    fn truncated_login_uuid_is_rejected_without_consuming_input() {
        let mut connection =
            PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
        enter_login(&mut connection);
        let truncated = body(
            generated::login_26_2::login::serverbound::LOGIN_HELLO,
            |writer| {
                writer.write_string("Player", 16)?;
                writer.write_u64(0)?;
                writer.write_bytes(&[0_u8; 7])
            },
        );
        connection
            .ingest(&encoded_frame(&truncated, limits()))
            .expect("truncated hello ingress");
        let buffered = connection.buffered_ingress();
        assert_eq!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Target(Target26_2Error::Codec(
                PacketCodecError::Truncated {
                    field: PacketField::U64,
                    remaining: 7,
                }
            )))
        );
        assert!(!connection.target_state().login_finished_sent());
        assert_eq!(connection.buffered_ingress(), buffered);
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
        assert_eq!(
            connection.pending_egress(),
            encoded_frame(&expected, limits())
        );
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

    #[test]
    fn egress_rejection_rolls_back_login_stage_and_input() {
        let limits = tight_limits();
        let mut connection =
            PrePlayConnection::<Target26_2>::with_target_state(limits, login_state());
        connection
            .ingest(
                generated::login_26_2::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_LOGIN_FRAME,
            )
            .expect("small-limit Login handshake ingress");
        connection
            .process_one(ORACLE_STATUS_JSON)
            .expect("small-limit Login handshake commits");

        connection
            .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_HELLO_FRAME)
            .expect("Login hello ingress");
        let buffered = connection.buffered_ingress();
        assert!(matches!(
            connection.process_one(ORACLE_STATUS_JSON),
            Err(PrePlayError::Buffer(_))
        ));
        assert_eq!(connection.phase(), SessionPhase::Login);
        assert!(!connection.target_state().login_finished_sent());
        assert_eq!(connection.buffered_ingress(), buffered);
        assert_eq!(connection.queued_egress(), 0);
    }
}
