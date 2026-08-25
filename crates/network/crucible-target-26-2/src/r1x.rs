//! Explicitly experimental Minecraft Java 26.2 join-replay target.
//!
//! This target is intentionally separate from [`crate::Target26_2`]. It composes the admitted
//! Handshake/Login implementation with the source-admitted Configuration route and, after the real
//! Configuration acknowledgement, can publish a source-free captured Play prefix for client smoke
//! testing. Captured Play traffic is not production evidence and must never silently become the
//! normal target's Play implementation.
//!
//! Runtime shape:
//!
//! ```text
//! process-owned immutable Configuration/Play bodies
//!                 +
//! compact Copy per-connection stage/cursors
//!                 +
//! existing bounded PrePlayPublisher egress
//! ```
//!
//! There is no runtime packet registry, no second outbound queue and no per-connection registry/NBT
//! reconstruction.

use std::fmt;

use crucible_connection_core::FrameView;
use crucible_connection_driver::OutboundBatch;
use crucible_packet_core::{PacketCodecError, PacketReader};
use crucible_preplay_core::{
    PrePlayAction, PrePlayPublication, PrePlayPublisher, PrePlayTarget, PublicationCursor,
    PublicationStep,
};
use crucible_session_core::{SessionPhase, SessionState, SessionStateError};

use crate::{LoginState, Target26_2, Target26_2Action, Target26_2Error, Target26_2State};

const CONFIGURATION_BODY_COUNT: usize = 34;
const CONFIGURATION_BODY_BYTES: usize = 44_432;
const CONFIGURATION_ENTRY_END: usize = 3;
const CONFIGURATION_REGISTRY_END: usize = 33;
const PLAY_BODY_LIMIT: usize = 65_536;
const PLAY_FULL_FRAME_COUNT: usize = 2_331;
const PLAY_FULL_BODY_BYTES: usize = 6_135_522;

const CONFIGURATION_BODY_SIZES: [usize; CONFIGURATION_BODY_COUNT] = [
    25, 20, 22, 1_612, 224, 327, 227, 184, 149, 77, 80, 78, 233, 66, 66, 77, 70, 81, 73, 980, 282,
    116, 1_143, 1_036, 968, 416, 237, 48, 49, 94, 64, 103, 35_204, 1,
];

const EXPECTED_PLAYER_NAME: &str = "Stato16";
const EXPECTED_OFFLINE_UUID: [u8; 16] = [
    0x68, 0x20, 0x14, 0xfe, 0xad, 0x63, 0x36, 0x99, 0xaa, 0xda, 0x79, 0xaa, 0x08, 0xd9, 0x5b, 0x45,
];

const CONFIG_SERVERBOUND_CLIENT_INFORMATION: i32 = 0;
const CONFIG_SERVERBOUND_CUSTOM_PAYLOAD: i32 = 2;
const CONFIG_SERVERBOUND_FINISH: i32 = 3;
const CONFIG_SERVERBOUND_SELECT_KNOWN_PACKS: i32 = 7;

const MAX_DEFAULT_UTF16_UNITS: usize = 32_767;
const MAX_LANGUAGE_UTF16_UNITS: usize = 16;
const MAX_LANGUAGE_UTF8_BYTES: usize = 48;
const CHAT_VISIBILITY_VARIANTS: i32 = 3;
const HUMANOID_ARM_VARIANTS: i32 = 2;
const PARTICLE_STATUS_VARIANTS: i32 = 3;

/// Cold-path validation failure while constructing the immutable R1X image context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R1xContextError {
    /// Configuration image has the wrong body count.
    ConfigurationBodyCount { observed: usize },
    /// One Configuration body length differs from the sealed capture image.
    ConfigurationBodyLength {
        index: usize,
        observed: usize,
        expected: usize,
    },
    /// One Configuration body has the wrong selected-route packet id.
    ConfigurationPacketId {
        index: usize,
        observed: Option<u8>,
        expected: u8,
    },
    /// Aggregate Configuration body bytes differ from the sealed image.
    ConfigurationBodyBytes { observed: usize },
    /// The selected Play prefix contains more bodies than the pinned full capture.
    PlayBodyCount { observed: usize },
    /// One captured Play body is empty or exceeds the explicit R1X frame-body bound.
    PlayBodyLength { index: usize, observed: usize },
    /// Aggregate captured Play bytes exceed the pinned full capture.
    PlayBodyBytes { observed: usize },
}

/// Process-owned immutable context for the experimental join target.
pub struct Target26_2R1xContext {
    status_json: Box<str>,
    configuration: Box<[Box<[u8]>]>,
    play: Box<[Box<[u8]>]>,
    play_body_bytes: usize,
}

impl Target26_2R1xContext {
    /// Builds a validated immutable R1X context.
    ///
    /// Cryptographic source/capture commitments are checked by the cold packer and runtime image
    /// loader. This constructor additionally seals the exact Configuration body layout and explicit
    /// Play bounds at the target boundary.
    ///
    /// # Errors
    ///
    /// Returns a fail-closed structural image error.
    pub fn new(
        status_json: Box<str>,
        configuration: Vec<Box<[u8]>>,
        play: Vec<Box<[u8]>>,
    ) -> Result<Self, R1xContextError> {
        validate_configuration(&configuration)?;
        let play_body_bytes = validate_play(&play)?;
        Ok(Self {
            status_json,
            configuration: configuration.into_boxed_slice(),
            play: play.into_boxed_slice(),
            play_body_bytes,
        })
    }

    /// Number of captured Play bodies selected for this smoke run.
    #[must_use]
    pub fn play_frame_count(&self) -> usize {
        self.play.len()
    }

    /// Aggregate selected captured Play body bytes.
    #[must_use]
    pub const fn play_body_bytes(&self) -> usize {
        self.play_body_bytes
    }

    fn status_json(&self) -> &str {
        &self.status_json
    }

    fn configuration_entry(&self) -> &[Box<[u8]>] {
        &self.configuration[..CONFIGURATION_ENTRY_END]
    }

    fn configuration_registry_and_tags(&self) -> &[Box<[u8]>] {
        &self.configuration[CONFIGURATION_ENTRY_END..CONFIGURATION_REGISTRY_END]
    }

    fn configuration_finish(&self) -> &[Box<[u8]>] {
        &self.configuration[CONFIGURATION_REGISTRY_END..]
    }

    fn play(&self) -> &[Box<[u8]>] {
        &self.play
    }
}

impl fmt::Debug for Target26_2R1xContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Target26_2R1xContext")
            .field("configuration_frames", &self.configuration.len())
            .field("configuration_body_bytes", &CONFIGURATION_BODY_BYTES)
            .field("play_frames", &self.play.len())
            .field("play_body_bytes", &self.play_body_bytes)
            .finish_non_exhaustive()
    }
}

fn validate_configuration(configuration: &[Box<[u8]>]) -> Result<(), R1xContextError> {
    if configuration.len() != CONFIGURATION_BODY_COUNT {
        return Err(R1xContextError::ConfigurationBodyCount {
            observed: configuration.len(),
        });
    }

    let mut total = 0_usize;
    for (index, (body, expected)) in configuration
        .iter()
        .zip(CONFIGURATION_BODY_SIZES)
        .enumerate()
    {
        if body.len() != expected {
            return Err(R1xContextError::ConfigurationBodyLength {
                index,
                observed: body.len(),
                expected,
            });
        }
        total = total
            .checked_add(body.len())
            .ok_or(R1xContextError::ConfigurationBodyBytes {
                observed: usize::MAX,
            })?;

        let expected_packet_id = match index {
            0 => 1,
            1 => 12,
            2 => 14,
            3..=31 => 7,
            32 => 13,
            33 => 3,
            _ => unreachable!("Configuration body count is sealed"),
        };
        let observed = body.first().copied();
        if observed != Some(expected_packet_id) {
            return Err(R1xContextError::ConfigurationPacketId {
                index,
                observed,
                expected: expected_packet_id,
            });
        }
    }

    if total != CONFIGURATION_BODY_BYTES {
        return Err(R1xContextError::ConfigurationBodyBytes { observed: total });
    }
    Ok(())
}

fn validate_play(play: &[Box<[u8]>]) -> Result<usize, R1xContextError> {
    if play.len() > PLAY_FULL_FRAME_COUNT {
        return Err(R1xContextError::PlayBodyCount {
            observed: play.len(),
        });
    }
    let mut total = 0_usize;
    for (index, body) in play.iter().enumerate() {
        if body.is_empty() || body.len() > PLAY_BODY_LIMIT {
            return Err(R1xContextError::PlayBodyLength {
                index,
                observed: body.len(),
            });
        }
        total = total
            .checked_add(body.len())
            .ok_or(R1xContextError::PlayBodyBytes {
                observed: usize::MAX,
            })?;
    }
    if total > PLAY_FULL_BODY_BYTES {
        return Err(R1xContextError::PlayBodyBytes { observed: total });
    }
    Ok(total)
}

/// Compact latest ClientInformation retained across Configuration.
///
/// The language is inline because target state is deliberately Copy and allocation-free. The
/// admitted 16-UTF-16-unit bound implies at most 48 UTF-8 bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ClientInformation {
    language: [u8; MAX_LANGUAGE_UTF8_BYTES],
    language_len: u8,
    view_distance: i8,
    chat_visibility: u8,
    chat_colors: bool,
    model_customisation: u8,
    main_hand: u8,
    text_filtering: bool,
    allows_listing: bool,
    particle_status: u8,
}

impl ClientInformation {
    const fn default_for_configuration() -> Self {
        let mut language = [0_u8; MAX_LANGUAGE_UTF8_BYTES];
        language[0] = b'e';
        language[1] = b'n';
        language[2] = b'_';
        language[3] = b'u';
        language[4] = b's';
        Self {
            language,
            language_len: 5,
            view_distance: 2,
            chat_visibility: 0,
            chat_colors: true,
            model_customisation: 0,
            main_hand: 1,
            text_filtering: false,
            allows_listing: false,
            particle_status: 0,
        }
    }

    fn decode(payload: &[u8]) -> Result<Self, R1xError> {
        let mut reader = PacketReader::new(payload);
        let language = reader.read_string(MAX_LANGUAGE_UTF16_UNITS)?;
        if language.len() > MAX_LANGUAGE_UTF8_BYTES {
            return Err(R1xError::InvalidClientInformation);
        }
        let view_distance = read_i8(&mut reader)?;
        let chat_visibility = read_enum(&mut reader, CHAT_VISIBILITY_VARIANTS)?;
        let chat_colors = reader.read_bool()?;
        let model_customisation = read_u8(&mut reader)?;
        let main_hand = read_enum(&mut reader, HUMANOID_ARM_VARIANTS)?;
        let text_filtering = reader.read_bool()?;
        let allows_listing = reader.read_bool()?;
        let particle_status = read_enum(&mut reader, PARTICLE_STATUS_VARIANTS)?;
        reader.finish()?;

        let mut stored_language = [0_u8; MAX_LANGUAGE_UTF8_BYTES];
        stored_language[..language.len()].copy_from_slice(language.as_bytes());
        Ok(Self {
            language: stored_language,
            language_len: u8::try_from(language.len())
                .map_err(|_| R1xError::InvalidClientInformation)?,
            view_distance,
            chat_visibility,
            chat_colors,
            model_customisation,
            main_hand,
            text_filtering,
            allows_listing,
            particle_status,
        })
    }
}

// PacketReader intentionally exposes only the generic primitives R0/R1A needed so far. These two
// one-byte Configuration fields stay local to the target rather than widening packet-core solely
// for this experimental slice. Rebuilding the reader over the borrowed tail remains allocation-free.
fn read_u8<'a>(reader: &mut PacketReader<'a>) -> Result<u8, R1xError> {
    let remaining = reader.read_remaining();
    let Some((&value, tail)) = remaining.split_first() else {
        return Err(R1xError::InvalidClientInformation);
    };
    *reader = PacketReader::new(tail);
    Ok(value)
}

fn read_i8<'a>(reader: &mut PacketReader<'a>) -> Result<i8, R1xError> {
    Ok(i8::from_be_bytes([read_u8(reader)?]))
}

fn read_enum(reader: &mut PacketReader<'_>, variants: i32) -> Result<u8, R1xError> {
    let ordinal = reader.read_var_int()?;
    if !(0..variants).contains(&ordinal) {
        return Err(R1xError::InvalidClientInformation);
    }
    u8::try_from(ordinal).map_err(|_| R1xError::InvalidClientInformation)
}

/// R1X Configuration publication state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigurationStage {
    Dormant,
    PublishEntry(PublicationCursor),
    AwaitKnownPacks,
    PublishRegistry(PublicationCursor),
    PublishFinish(PublicationCursor),
    AwaitFinishAcknowledgement,
    Complete,
}

/// Compact transactional target state for the experimental join target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Target26_2R1xState {
    base: Target26_2State,
    configuration: ConfigurationStage,
    client_information: ClientInformation,
    play_cursor: PublicationCursor,
    play_complete: bool,
}

impl Default for Target26_2R1xState {
    fn default() -> Self {
        Self {
            base: Target26_2State::default(),
            configuration: ConfigurationStage::Dormant,
            client_information: ClientInformation::default_for_configuration(),
            play_cursor: PublicationCursor::new(),
            play_complete: false,
        }
    }
}

impl Target26_2R1xState {
    /// Starts the admitted Login route using the supplied deterministic session UUID.
    #[must_use]
    pub const fn with_login_session_uuid(session_uuid: [u8; 16]) -> Self {
        Self {
            base: Target26_2State::with_login_session_uuid(session_uuid),
            configuration: ConfigurationStage::Dormant,
            client_information: ClientInformation::default_for_configuration(),
            play_cursor: PublicationCursor::new(),
            play_complete: false,
        }
    }

    /// Whether the selected captured Play prefix has fully entered bounded egress.
    #[must_use]
    pub const fn replay_complete(self) -> bool {
        self.play_complete
    }

    /// Number of selected Play bodies already admitted to bounded egress.
    #[must_use]
    pub const fn replay_cursor(self) -> usize {
        self.play_cursor.next_index()
    }
}

/// Explicit R1X target marker.
///
/// This target exists only for development smoke joins. Production 26.2 semantics remain on
/// [`Target26_2`] and cannot accidentally inherit permissive Play replay behavior.
#[derive(Clone, Copy, Debug, Default)]
pub struct Target26_2R1x;

/// R1X target failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R1xError {
    /// Admitted base Handshake/Status/Login target rejected the packet.
    Base(Target26_2Error),
    /// Configuration packet appeared before the R1X entry publication became active.
    ConfigurationNotStarted,
    /// Selected known-pack response did not match the one immutable registry image.
    KnownPackSelectionMismatch,
    /// ClientInformation violated the admitted field law.
    InvalidClientInformation,
    /// Finish acknowledgement arrived before the clientbound finish publication completed.
    PrematureFinishAcknowledgement,
    /// Captured Play replay is profile-specific and the accepted Login profile did not match it.
    ReplayProfileMismatch,
    /// Generic packet-body codec failure in source-admitted Configuration decoding.
    Codec(PacketCodecError),
    /// Session lifecycle transition failed.
    Transition(SessionStateError),
}

impl From<Target26_2Error> for R1xError {
    fn from(value: Target26_2Error) -> Self {
        Self::Base(value)
    }
}

impl From<PacketCodecError> for R1xError {
    fn from(value: PacketCodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<SessionStateError> for R1xError {
    fn from(value: SessionStateError) -> Self {
        Self::Transition(value)
    }
}

/// Transactional R1X action.
#[derive(Debug)]
pub struct R1xAction {
    candidate: SessionState,
    frames: Vec<Vec<u8>>,
    next_state: Target26_2R1xState,
}

impl OutboundBatch for R1xAction {
    type Body = Vec<u8>;

    fn outbound_frames(&self) -> &[Self::Body] {
        &self.frames
    }
}

impl PrePlayAction for R1xAction {
    fn candidate_session(&self) -> SessionState {
        self.candidate
    }
}

impl PrePlayTarget for Target26_2R1x {
    type Error = R1xError;
    type Context = Target26_2R1xContext;
    type State = Target26_2R1xState;
    type Action = R1xAction;

    fn decode(
        context: &Self::Context,
        session: SessionState,
        state: &Self::State,
        frame: FrameView<'_>,
    ) -> Result<Self::Action, Self::Error> {
        match session.phase() {
            SessionPhase::Handshake | SessionPhase::Status | SessionPhase::Login => {
                let action = <Target26_2 as PrePlayTarget>::decode(
                    context.status_json(),
                    session,
                    &state.base,
                    frame,
                )?;
                Ok(wrap_base_action(*state, action, session.phase()))
            }
            SessionPhase::Configuration => decode_configuration(session, *state, frame),
            SessionPhase::Play => Ok(R1xAction {
                // Captured Play is a black-box smoke fixture. Complete, already-bounded client Play
                // frames are consumed without interpretation only in this explicitly experimental
                // target so coalesced acknowledgements cannot stall the outbound replay.
                candidate: session,
                frames: Vec::new(),
                next_state: *state,
            }),
            SessionPhase::Closed => Err(R1xError::Base(Target26_2Error::UnsupportedPhase(
                SessionPhase::Closed,
            ))),
        }
    }

    fn commit_target_state(state: &mut Self::State, action: Self::Action) {
        *state = action.next_state;
    }
}

fn wrap_base_action(
    state: Target26_2R1xState,
    action: Target26_2Action,
    phase_before: SessionPhase,
) -> R1xAction {
    let Target26_2Action {
        candidate,
        frames,
        next_state,
    } = action;
    let mut wrapped = state;
    wrapped.base = next_state;
    if phase_before == SessionPhase::Login && candidate.phase() == SessionPhase::Configuration {
        wrapped.configuration = ConfigurationStage::PublishEntry(PublicationCursor::new());
    }
    R1xAction {
        candidate,
        frames,
        next_state: wrapped,
    }
}

fn decode_configuration(
    session: SessionState,
    state: Target26_2R1xState,
    frame: FrameView<'_>,
) -> Result<R1xAction, R1xError> {
    if state.configuration == ConfigurationStage::Dormant {
        return Err(R1xError::ConfigurationNotStarted);
    }

    let mut next = state;
    match frame.packet_id() {
        CONFIG_SERVERBOUND_CUSTOM_PAYLOAD => {
            decode_brand_payload(frame.payload())?;
        }
        CONFIG_SERVERBOUND_CLIENT_INFORMATION => {
            next.client_information = ClientInformation::decode(frame.payload())?;
        }
        CONFIG_SERVERBOUND_SELECT_KNOWN_PACKS => {
            if state.configuration != ConfigurationStage::AwaitKnownPacks {
                return Err(R1xError::KnownPackSelectionMismatch);
            }
            decode_selected_known_pack(frame.payload())?;
            next.configuration = ConfigurationStage::PublishRegistry(PublicationCursor::new());
        }
        CONFIG_SERVERBOUND_FINISH => {
            if state.configuration != ConfigurationStage::AwaitFinishAcknowledgement {
                return Err(R1xError::PrematureFinishAcknowledgement);
            }
            let reader = PacketReader::new(frame.payload());
            reader.finish()?;
            validate_replay_profile(state.base)?;
            let mut candidate = session;
            candidate.advance(SessionPhase::Play)?;
            next.configuration = ConfigurationStage::Complete;
            return Ok(R1xAction {
                candidate,
                frames: Vec::new(),
                next_state: next,
            });
        }
        packet_id => {
            return Err(R1xError::Base(Target26_2Error::UnknownPacket {
                phase: SessionPhase::Configuration,
                packet_id,
            }));
        }
    }

    Ok(R1xAction {
        candidate: session,
        frames: Vec::new(),
        next_state: next,
    })
}

fn decode_brand_payload(payload: &[u8]) -> Result<(), R1xError> {
    let mut reader = PacketReader::new(payload);
    let identifier = reader.read_string(MAX_DEFAULT_UTF16_UNITS)?;
    if identifier != "minecraft:brand" {
        return Err(R1xError::Base(Target26_2Error::UnknownPacket {
            phase: SessionPhase::Configuration,
            packet_id: CONFIG_SERVERBOUND_CUSTOM_PAYLOAD,
        }));
    }
    let _brand = reader.read_string(MAX_DEFAULT_UTF16_UNITS)?;
    reader.finish()?;
    Ok(())
}

fn decode_selected_known_pack(payload: &[u8]) -> Result<(), R1xError> {
    let mut reader = PacketReader::new(payload);
    if reader.read_var_int()? != 1
        || reader.read_string(MAX_DEFAULT_UTF16_UNITS)? != "minecraft"
        || reader.read_string(MAX_DEFAULT_UTF16_UNITS)? != "core"
        || reader.read_string(MAX_DEFAULT_UTF16_UNITS)? != "26.2"
    {
        return Err(R1xError::KnownPackSelectionMismatch);
    }
    reader.finish()?;
    Ok(())
}

fn validate_replay_profile(base: Target26_2State) -> Result<(), R1xError> {
    let LoginState::AwaitAcknowledgement { profile, .. } = base.login else {
        return Err(R1xError::ReplayProfileMismatch);
    };
    if profile.id() != EXPECTED_OFFLINE_UUID || profile.name() != EXPECTED_PLAYER_NAME {
        return Err(R1xError::ReplayProfileMismatch);
    }
    Ok(())
}

/// Public only because [`PrePlayPublisher`] exposes its commit token as an associated type.
///
/// R1X callers should never construct or branch on this token; it is hidden plumbing between the
/// target proposal and the target-neutral publication binder.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R1xPublicationCommit {
    ConfigurationEntry,
    ConfigurationRegistry,
    ConfigurationFinish,
    PlayReplay,
}

impl PrePlayPublisher for Target26_2R1x {
    type PublicationBody = Box<[u8]>;
    type PublicationCommit = R1xPublicationCommit;

    fn publication<'a>(
        context: &'a Self::Context,
        session: SessionState,
        state: &'a Self::State,
    ) -> Result<
        Option<PrePlayPublication<'a, Self::PublicationBody, Self::PublicationCommit>>,
        Self::Error,
    > {
        let proposal = match (session.phase(), state.configuration) {
            (SessionPhase::Configuration, ConfigurationStage::PublishEntry(cursor)) => {
                Some(PrePlayPublication::new(
                    context.configuration_entry(),
                    cursor,
                    R1xPublicationCommit::ConfigurationEntry,
                ))
            }
            (SessionPhase::Configuration, ConfigurationStage::PublishRegistry(cursor)) => {
                Some(PrePlayPublication::new(
                    context.configuration_registry_and_tags(),
                    cursor,
                    R1xPublicationCommit::ConfigurationRegistry,
                ))
            }
            (SessionPhase::Configuration, ConfigurationStage::PublishFinish(cursor)) => {
                Some(PrePlayPublication::new(
                    context.configuration_finish(),
                    cursor,
                    R1xPublicationCommit::ConfigurationFinish,
                ))
            }
            (SessionPhase::Play, ConfigurationStage::Complete) if !state.play_complete => {
                Some(PrePlayPublication::new(
                    context.play(),
                    state.play_cursor,
                    R1xPublicationCommit::PlayReplay,
                ))
            }
            _ => None,
        };
        Ok(proposal)
    }

    fn commit_publication(
        state: &mut Self::State,
        commit: Self::PublicationCommit,
        cursor: PublicationCursor,
        step: PublicationStep,
    ) {
        match commit {
            R1xPublicationCommit::ConfigurationEntry => {
                state.configuration = if matches!(step, PublicationStep::Complete) {
                    ConfigurationStage::AwaitKnownPacks
                } else {
                    ConfigurationStage::PublishEntry(cursor)
                };
            }
            R1xPublicationCommit::ConfigurationRegistry => {
                state.configuration = if matches!(step, PublicationStep::Complete) {
                    ConfigurationStage::PublishFinish(PublicationCursor::new())
                } else {
                    ConfigurationStage::PublishRegistry(cursor)
                };
            }
            R1xPublicationCommit::ConfigurationFinish => {
                state.configuration = if matches!(step, PublicationStep::Complete) {
                    ConfigurationStage::AwaitFinishAcknowledgement
                } else {
                    ConfigurationStage::PublishFinish(cursor)
                };
            }
            R1xPublicationCommit::PlayReplay => {
                state.play_cursor = cursor;
                if matches!(step, PublicationStep::Complete) {
                    state.play_complete = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crucible_preplay_core::{PrePlayPublisher, PublicationCursor, PublicationStep};

    use super::{
        CONFIGURATION_BODY_SIZES, CONFIGURATION_ENTRY_END, CONFIGURATION_REGISTRY_END,
        ConfigurationStage, R1xContextError, R1xPublicationCommit, Target26_2R1x,
        Target26_2R1xContext, Target26_2R1xState,
    };

    fn dummy_context(play_frames: usize) -> Target26_2R1xContext {
        let mut configuration = Vec::new();
        for (index, size) in CONFIGURATION_BODY_SIZES.into_iter().enumerate() {
            let packet_id = match index {
                0 => 1,
                1 => 12,
                2 => 14,
                3..=31 => 7,
                32 => 13,
                33 => 3,
                _ => unreachable!(),
            };
            let mut body = vec![0_u8; size];
            body[0] = packet_id;
            configuration.push(body.into_boxed_slice());
        }
        let play = (0..play_frames)
            .map(|index| vec![u8::try_from(index % 0x7f).expect("bounded id")].into_boxed_slice())
            .collect();
        Target26_2R1xContext::new("{}".into(), configuration, play).expect("valid dummy image")
    }

    #[test]
    fn context_seals_configuration_layout_and_play_bounds() {
        let context = dummy_context(2);
        assert_eq!(context.play_frame_count(), 2);
        assert_eq!(context.play_body_bytes(), 2);

        let mut bad = context.configuration.into_vec();
        bad[CONFIGURATION_ENTRY_END][0] = 8;
        let error = Target26_2R1xContext::new("{}".into(), bad, Vec::new())
            .expect_err("wrong registry packet id");
        assert!(matches!(
            error,
            R1xContextError::ConfigurationPacketId {
                index: CONFIGURATION_ENTRY_END,
                ..
            }
        ));

        let context = dummy_context(0);
        assert_eq!(
            context.configuration_registry_and_tags().len(),
            CONFIGURATION_REGISTRY_END - CONFIGURATION_ENTRY_END
        );
    }

    #[test]
    fn client_information_accepts_non_capture_settings_without_allocation() {
        let payload = [
            0x05, b'f', b'r', b'_', b'f', b'r', // language
            0x08, // view distance
            0x02, // hidden chat
            0x00, // colors
            0x55, // model customisation
            0x00, // left hand
            0x01, // filtering
            0x00, // listing
            0x02, // minimal particles
        ];
        let decoded = super::ClientInformation::decode(&payload).expect("valid settings");
        assert_eq!(decoded.view_distance, 8);
        assert_eq!(decoded.chat_visibility, 2);
        assert_eq!(decoded.model_customisation, 0x55);
        assert_eq!(decoded.main_hand, 0);
        assert_eq!(decoded.particle_status, 2);
    }

    #[test]
    fn known_pack_branch_is_exactly_the_selected_core_pack() {
        let mut payload = vec![1];
        for value in ["minecraft", "core", "26.2"] {
            payload.push(u8::try_from(value.len()).expect("selected strings fit one-byte VarInt"));
            payload.extend_from_slice(value.as_bytes());
        }
        assert_eq!(super::decode_selected_known_pack(&payload), Ok(()));

        payload[0] = 0;
        assert!(super::decode_selected_known_pack(&payload).is_err());
    }

    #[test]
    fn empty_selected_play_prefix_completes_without_a_synthetic_frame() {
        let mut state = Target26_2R1xState::default();
        state.configuration = ConfigurationStage::Complete;
        <Target26_2R1x as PrePlayPublisher>::commit_publication(
            &mut state,
            R1xPublicationCommit::PlayReplay,
            PublicationCursor::new(),
            PublicationStep::Complete,
        );
        assert!(state.replay_complete());
        assert_eq!(state.replay_cursor(), 0);
    }
}
