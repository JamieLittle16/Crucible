//! Source-admitted Minecraft Java 26.2 pre-play target for Crucible.
//!
//! This crate is the target-version semantic layer above Crucible's target-neutral packet,
//! connection and pre-play machinery. It currently implements two independently admitted finite
//! surfaces:
//!
//! - R0: Handshake -> Status -> Status response -> Ping/Pong;
//! - R1A: Handshake(LOGIN) -> offline Hello -> `LoginFinished` -> `LoginAcknowledged` -> Configuration.
//!
//! Packet identities are generated from admitted contracts. Runtime dispatch is direct static
//! matching; there is no packet registry, target lookup, trait object, socket runtime or second
//! framing/buffering layer here.

#![forbid(unsafe_code)]

mod login_profile;
mod offline_uuid;

use crucible_connection_core::FrameView;
use crucible_connection_driver::OutboundBatch;
use crucible_packet_core::{PacketCodecError, PacketReader, PacketWriter};
use crucible_preplay_core::{PrePlayAction, PrePlayTarget};
use crucible_session_core::{SessionPhase, SessionState, SessionStateError};
use login_profile::LoginProfile;

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
/// The source-valid player-name predicate narrows the already-16-unit codec field to printable
/// ASCII, so the maximum body is `id(1) + profile UUID(16) + name length(1) + name(16) + property
/// count(1) + session UUID(16)`.
pub const MAX_R1A_LOGIN_PACKET_BODY_BYTES: usize = 51;

const STATUS_INTENT: i32 = 1;
const LOGIN_INTENT: i32 = 2;

/// Fine-grained Login state carried only by the 26.2 target.
///
/// Each variant owns exactly the data valid at that point, preventing partial combinations such as
/// an accepted profile without a session UUID. After acknowledgement the generic session moves to
/// Configuration while this value remains `AwaitAcknowledgement`, retaining the immutable accepted
/// profile for the Configuration handoff rather than duplicating the coarse phase bit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum LoginState {
    /// Status-only/default connection; Login is not product-enabled.
    #[default]
    Disabled,
    /// Login is enabled and waiting for the first source-admitted hello.
    AwaitHello { session_uuid: [u8; 16] },
    /// `LoginFinished` committed; the accepted profile must survive into Configuration.
    AwaitAcknowledgement {
        session_uuid: [u8; 16],
        profile: LoginProfile,
    },
}

/// Per-connection 26.2 state finer than the generic [`SessionPhase`].
///
/// Status-only product composition may use [`Default`]. Login-capable product composition supplies
/// the runtime server connection session UUID explicitly through [`Self::with_login_session_uuid`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Target26_2State {
    status_response_sent: bool,
    login: LoginState,
}

impl Target26_2State {
    /// Creates target-local state capable of the admitted R1A Login route.
    #[must_use]
    pub const fn with_login_session_uuid(session_uuid: [u8; 16]) -> Self {
        Self {
            status_response_sent: false,
            login: LoginState::AwaitHello { session_uuid },
        }
    }

    /// Whether the one allowed status response has already committed on this connection.
    #[must_use]
    pub const fn status_response_sent(&self) -> bool {
        self.status_response_sent
    }

    /// Whether `LoginFinished` has committed and the accepted profile is retained.
    #[must_use]
    pub const fn login_finished_sent(&self) -> bool {
        matches!(self.login, LoginState::AwaitAcknowledgement { .. })
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
    /// Login needs no product service object: its per-connection state is target-local and inline.
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
            if matches!(target_state.login, LoginState::Disabled) {
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

    match (target_state.login, frame.packet_id()) {
        (LoginState::AwaitHello { session_uuid }, LOGIN_HELLO) => {
            let mut reader = PacketReader::new(frame.payload());
            let player_name = reader.read_string(MAX_PLAYER_NAME_UTF16_UNITS)?;
            let _client_uuid_msb = reader.read_u64()?;
            let _client_uuid_lsb = reader.read_u64()?;
            reader.finish()?;

            if !valid_player_name(player_name) {
                return Err(Target26_2Error::InvalidPlayerName);
            }

            let profile =
                LoginProfile::new(offline_uuid::offline_player_uuid(player_name), player_name);
            let response = encode_login_finished(profile.name(), profile.id(), session_uuid)?;

            let mut next_state = target_state;
            next_state.login = LoginState::AwaitAcknowledgement {
                session_uuid,
                profile,
            };
            Ok(Target26_2Action::new(session, vec![response], next_state))
        }
        (LoginState::AwaitAcknowledgement { .. }, LOGIN_ACKNOWLEDGED) => {
            let reader = PacketReader::new(frame.payload());
            reader.finish()?;

            let mut candidate = session;
            candidate.advance(SessionPhase::Configuration)?;
            Ok(Target26_2Action::new(candidate, Vec::new(), target_state))
        }
        (_, LOGIN_HELLO | LOGIN_ACKNOWLEDGED) => Err(Target26_2Error::UnexpectedLoginState),
        (_, packet_id) => Err(Target26_2Error::UnknownPacket {
            phase: SessionPhase::Login,
            packet_id,
        }),
    }
}

fn valid_player_name(player_name: &str) -> bool {
    // Java 26.2 `StringUtil.isValidPlayerName`: after the codec's <=16 UTF-16-unit bound, reject
    // any UTF-16 unit <=32 or >=127. The surviving strings are therefore exactly printable ASCII;
    // the source predicate intentionally also accepts the empty string.
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
mod tests;
