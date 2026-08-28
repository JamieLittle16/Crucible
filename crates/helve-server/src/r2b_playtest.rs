//! Stock-client development composition for the replay-free R2B boundary.
//!
//! The cold image contains source-admitted Configuration plus exactly three immutable shared R2B
//! projections. It has no captured-Play publication section. After `WorldProjectionReady`, this
//! temporary owner keeps the exact R2B driver/read scratch, claims teleport and keep-alive normally,
//! and discards currently-unowned gameplay frames only until R2C exists.

use std::convert::Infallible;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use helve_connection_driver::{DriverError, OutboundBatch, TransactionResult};
use helve_target_26_2::{
    R1xContextError, Target26_2R1xContext,
    play_liveness::PLAY_LIVENESS_POLICY,
    r2b::{
        BootstrapGameMode, BootstrapWeather, ChangeDifficultyPayload, ClockFullSyncPayload,
        ClockUpdate, CommandPermissionProfile, CommandProjectionArtifact, CommandProjectionKey,
        DefaultSpawnPayload, Difficulty26_2, FreshCommonSpawnInfo, FreshEmptyInventoryPayload,
        FreshLoginFlags, FreshLoginPayload, FreshR2bBootstrapSnapshot, HeldSlotPayload,
        InitialPlayerInfoEntry, PermissionEntityEventPayload, PermissionLevelEvent,
        PlayBootstrapImage26_2, PlayerAbilitiesPayload, PlayerAbilityFlags,
        ProjectionArtifactError, ProjectionRevision, RecipeBookSettingFlags,
        RecipeBookSettingsPayload, RecipeProjectionArtifact, RecipeProjectionKey,
        ServerDataProjection, ServerDataProjectionArtifact, ServerDataProjectionKey,
        TeleportAckResult, TeleportDestination, TickingStatePayload, TickingStepPayload,
        WorldBorderPayload,
    },
};

use crate::{
    R2bEntryOutcome, R2bLivenessProcess, R2bPlayError, R2bPlayInbound, R2bPlayProcess,
    R2bPlaySession, R2bServerError, ServerSessionEpoch, enter_r2b_play_blocking_transport,
};

const MAGIC: [u8; 8] = *b"CRR2B001";
const EXPECTED_PROTOCOL: u32 = 776;
const EXPECTED_CONFIGURATION_COUNT: usize = 34;
const EXPECTED_CONFIGURATION_BYTES: usize = 44_430;
const MAX_BODY_BYTES: usize = 65_536;
const HEADER_BYTES: u64 = 88;
const LENGTH_PREFIX_BYTES: u64 = 37 * 4;
const MAX_SHARED_BODY_BYTES: u64 = 3 * 65_536;
const MAX_IMAGE_BYTES: u64 = HEADER_BYTES
    + LENGTH_PREFIX_BYTES
    + EXPECTED_CONFIGURATION_BYTES as u64
    + MAX_SHARED_BODY_BYTES;
const ACTIONS_PER_SERVICE: usize = 32;
const _: () = assert!(ACTIONS_PER_SERVICE > 0);

const EXPECTED_SOURCE_SHA256: [u8; 32] = [
    0x1e, 0x9b, 0xca, 0x3d, 0xff, 0x83, 0xcd, 0x83, 0xe7, 0x90, 0x5f, 0x88, 0x10, 0xf1, 0xec, 0x98,
    0x99, 0x36, 0x1f, 0xa2, 0xdc, 0x83, 0xfe, 0x89, 0x3b, 0xb4, 0x8b, 0xee, 0xb0, 0x4d, 0xf7, 0x50,
];
const EXPECTED_CAPTURE_SHA256: [u8; 32] = [
    0x11, 0xea, 0xd8, 0xde, 0x74, 0xdf, 0x70, 0xb4, 0x0d, 0x7f, 0xb0, 0x45, 0xff, 0x95, 0x61, 0xf0,
    0x6f, 0x6e, 0x24, 0x23, 0x87, 0x65, 0xd4, 0x14, 0x1a, 0x1d, 0x09, 0x0c, 0xab, 0x54, 0x6b, 0x57,
];

const LEVELS: [&str; 3] = [
    "minecraft:overworld",
    "minecraft:the_nether",
    "minecraft:the_end",
];
const CLOCKS: [ClockUpdate; 2] = [
    ClockUpdate {
        registry_id: 0,
        total_ticks: 5_503,
        partial_tick: 0.0,
        rate: 1.0,
    },
    ClockUpdate {
        registry_id: 1,
        total_ticks: 5_503,
        partial_tick: 0.0,
        rate: 1.0,
    },
];
const SELF: InitialPlayerInfoEntry<'static> = InitialPlayerInfoEntry {
    profile_id: [
        0x68, 0x20, 0x14, 0xfe, 0xad, 0x63, 0x36, 0x99, 0xaa, 0xda, 0x79, 0xaa, 0x08, 0xd9, 0x5b,
        0x45,
    ],
    name: "Stato16",
    game_mode: BootstrapGameMode::Survival,
    listed: true,
    latency: 0,
    list_order: 0,
    show_hat: true,
};

/// Cold-path R2B playtest-image failure.
#[derive(Debug)]
pub enum R2bPlaytestImageError {
    /// Filesystem access failed.
    Io {
        /// Stable I/O classification.
        kind: io::ErrorKind,
        /// Diagnostic detail.
        message: String,
    },
    /// Image path resolved to a symbolic link.
    Symlink,
    /// Image path was not a regular file.
    NotFile,
    /// File exceeds the maximum possible bounded playtest image.
    FileTooLarge { observed: u64, maximum: u64 },
    /// Header magic did not identify the R2B playtest format.
    Magic,
    /// Protocol differs from Minecraft Java 26.2.
    Protocol { observed: u32 },
    /// Source archive commitment differs from the pinned source archive.
    SourceCommitment,
    /// Capture commitment differs from the qualified selected capture.
    CaptureCommitment,
    /// Configuration count differs from the sealed selected route.
    ConfigurationCount { observed: usize },
    /// Configuration body-byte total differs from the selected runtime route.
    ConfigurationBytes { observed: usize },
    /// One body is empty or exceeds the finite packet-body bound.
    BodyLength { index: usize, observed: usize },
    /// Decoded Configuration aggregate differs from the header declaration.
    AggregateMismatch { declared: usize, observed: usize },
    /// Undeclared bytes remained after the fixed body sequence.
    TrailingData,
    /// Target-level Configuration validation rejected the image.
    Context(R1xContextError),
    /// One shared R2B projection carried a malformed or wrong packet identity.
    Projection(ProjectionArtifactError),
}

impl fmt::Display for R2bPlaytestImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { kind, message } => {
                write!(formatter, "playtest image I/O {kind:?}: {message}")
            }
            Self::Symlink => formatter.write_str("playtest image must not be a symlink"),
            Self::NotFile => formatter.write_str("playtest image must be a regular file"),
            Self::FileTooLarge { observed, maximum } => write!(
                formatter,
                "playtest image is {observed} bytes, exceeding the {maximum}-byte bound"
            ),
            Self::Magic => formatter.write_str("playtest image magic mismatch"),
            Self::Protocol { observed } => write!(
                formatter,
                "playtest image protocol mismatch: expected {EXPECTED_PROTOCOL}, got {observed}"
            ),
            Self::SourceCommitment => {
                formatter.write_str("playtest image source commitment mismatch")
            }
            Self::CaptureCommitment => {
                formatter.write_str("playtest image capture commitment mismatch")
            }
            Self::ConfigurationCount { observed } => write!(
                formatter,
                "playtest Configuration count mismatch: expected {EXPECTED_CONFIGURATION_COUNT}, got {observed}"
            ),
            Self::ConfigurationBytes { observed } => write!(
                formatter,
                "playtest Configuration bytes mismatch: expected {EXPECTED_CONFIGURATION_BYTES}, got {observed}"
            ),
            Self::BodyLength { index, observed } => {
                write!(
                    formatter,
                    "playtest body {index} has invalid {observed}-byte length"
                )
            }
            Self::AggregateMismatch { declared, observed } => write!(
                formatter,
                "playtest Configuration aggregate mismatch: declared {declared}, decoded {observed}"
            ),
            Self::TrailingData => formatter.write_str("playtest image contains trailing data"),
            Self::Context(error) => write!(formatter, "playtest Configuration rejected: {error:?}"),
            Self::Projection(error) => {
                write!(formatter, "playtest shared projection rejected: {error:?}")
            }
        }
    }
}

impl From<R1xContextError> for R2bPlaytestImageError {
    fn from(value: R1xContextError) -> Self {
        Self::Context(value)
    }
}

impl From<ProjectionArtifactError> for R2bPlaytestImageError {
    fn from(value: ProjectionArtifactError) -> Self {
        Self::Projection(value)
    }
}

/// Process-owned replay-free inputs for the selected stock-client R2B playtest.
#[derive(Debug)]
pub struct R2bPlaytestImage {
    configuration: Target26_2R1xContext,
    bootstrap: PlayBootstrapImage26_2,
    server_data: ServerDataProjectionArtifact,
}

/// Why the temporary post-R2B playtest owner ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R2bPlaytestExit {
    /// Connection ended before the explicit world-projection handoff.
    SessionClosedBeforeWorld,
    /// Peer closed before the explicit world-projection handoff.
    PeerEofBeforeWorld,
    /// Peer closed after reaching `WorldProjection`.
    LivePeerEof {
        /// Whether the initial absolute teleport was acknowledged exactly.
        teleport_acknowledged: bool,
        /// Valid keep-alive replies committed through the canonical R2B session.
        accepted_keep_alives: u64,
        /// Currently-unowned complete Play frames discarded by the temporary owner.
        discarded_world_frames: u64,
        /// Last source-compatible smoothed latency.
        latency_ms: i32,
    },
    /// Initial teleport acknowledgement was stale, wrong, duplicate, or unsolicited.
    InvalidTeleportAcknowledgement(TeleportAckResult),
    /// A keep-alive reply did not match the outstanding challenge.
    InvalidKeepAlive,
    /// One outstanding keep-alive survived through its next deadline.
    KeepAliveTimedOut,
    /// Closed-listener timeout fired unexpectedly in this development composition.
    ClosedTimedOut,
    /// Monotonic time exceeded the admitted wire domain.
    ClockInvalid,
}

/// Failure from the temporary R2B playtest outer owner.
#[derive(Debug)]
pub enum R2bPlaytestError {
    /// Canonical replay-free R2B entry failed.
    Entry(R2bServerError),
    /// Canonical continuing-Play control processing failed.
    Play(R2bPlayError),
    /// Direct same-driver discard/read accounting failed.
    Driver(DriverError<Infallible>),
    /// Transport I/O failed.
    Io {
        /// Read/write operation.
        operation: &'static str,
        /// Stable I/O classification.
        kind: io::ErrorKind,
        /// Diagnostic detail.
        message: String,
    },
    /// EOF arrived with an incomplete framed Play packet buffered.
    TruncatedEof { buffered_ingress: usize },
    /// An impossible incomplete frame followed an R2B `Unclaimed` classification.
    DiscardInvariant,
    /// A diagnostic counter overflowed.
    AccountingOverflow,
}

impl From<R2bServerError> for R2bPlaytestError {
    fn from(value: R2bServerError) -> Self {
        Self::Entry(value)
    }
}

impl From<R2bPlayError> for R2bPlaytestError {
    fn from(value: R2bPlayError) -> Self {
        Self::Play(value)
    }
}

/// Loads the compact replay-free R2B playtest image.
///
/// The fixed body sequence is Configuration, update-recipes, commands, server-data. There is no
/// captured-Play publication section.
///
/// # Errors
///
/// Rejects wrong format/commitments/bounds/trailing data, invalid Configuration, or malformed shared
/// projection packet identities.
pub fn load_r2b_playtest_image(
    path: &Path,
    status_json: &str,
) -> Result<R2bPlaytestImage, R2bPlaytestImageError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| image_io_error(&error))?;
    if metadata.file_type().is_symlink() {
        return Err(R2bPlaytestImageError::Symlink);
    }
    if !metadata.is_file() {
        return Err(R2bPlaytestImageError::NotFile);
    }
    if metadata.len() > MAX_IMAGE_BYTES {
        return Err(R2bPlaytestImageError::FileTooLarge {
            observed: metadata.len(),
            maximum: MAX_IMAGE_BYTES,
        });
    }
    let file = File::open(path).map_err(|error| image_io_error(&error))?;
    decode_image(&mut BufReader::new(file), status_json)
}

fn decode_image<R: Read>(
    reader: &mut R,
    status_json: &str,
) -> Result<R2bPlaytestImage, R2bPlaytestImageError> {
    if read_array::<8, _>(reader)? != MAGIC {
        return Err(R2bPlaytestImageError::Magic);
    }
    let protocol = read_u32(reader)?;
    if protocol != EXPECTED_PROTOCOL {
        return Err(R2bPlaytestImageError::Protocol { observed: protocol });
    }
    if read_array::<32, _>(reader)? != EXPECTED_SOURCE_SHA256 {
        return Err(R2bPlaytestImageError::SourceCommitment);
    }
    if read_array::<32, _>(reader)? != EXPECTED_CAPTURE_SHA256 {
        return Err(R2bPlaytestImageError::CaptureCommitment);
    }

    let configuration_count = usize_from_u32(read_u32(reader)?)?;
    let configuration_bytes = usize_from_u64(read_u64(reader)?)?;
    if configuration_count != EXPECTED_CONFIGURATION_COUNT {
        return Err(R2bPlaytestImageError::ConfigurationCount {
            observed: configuration_count,
        });
    }
    if configuration_bytes != EXPECTED_CONFIGURATION_BYTES {
        return Err(R2bPlaytestImageError::ConfigurationBytes {
            observed: configuration_bytes,
        });
    }

    let mut configuration = Vec::with_capacity(configuration_count);
    let mut observed_configuration_bytes = 0_usize;
    for index in 0..configuration_count {
        let body = read_body(reader, index)?;
        observed_configuration_bytes = observed_configuration_bytes.checked_add(body.len()).ok_or(
            R2bPlaytestImageError::AggregateMismatch {
                declared: configuration_bytes,
                observed: usize::MAX,
            },
        )?;
        configuration.push(body);
    }
    if observed_configuration_bytes != configuration_bytes {
        return Err(R2bPlaytestImageError::AggregateMismatch {
            declared: configuration_bytes,
            observed: observed_configuration_bytes,
        });
    }

    let recipes = read_body(reader, EXPECTED_CONFIGURATION_COUNT)?;
    let commands = read_body(reader, EXPECTED_CONFIGURATION_COUNT + 1)?;
    let server_data = read_body(reader, EXPECTED_CONFIGURATION_COUNT + 2)?;
    let mut trailing = [0_u8; 1];
    if reader
        .read(&mut trailing)
        .map_err(|error| image_io_error(&error))?
        != 0
    {
        return Err(R2bPlaytestImageError::TrailingData);
    }

    let configuration = Target26_2R1xContext::new(status_json.into(), configuration, Vec::new())?;
    debug_assert_eq!(configuration.play_frame_count(), 0);
    let bootstrap = PlayBootstrapImage26_2::new(
        CommandProjectionArtifact::new(command_key(), commands)?,
        RecipeProjectionArtifact::new(recipe_key(), recipes)?,
    );
    let server_data = ServerDataProjectionArtifact::new(status_key(), server_data)?;
    Ok(R2bPlaytestImage {
        configuration,
        bootstrap,
        server_data,
    })
}

/// Drives one stock-client connection through real Configuration, replay-free R2B and the temporary
/// post-`WorldProjection` owner.
///
/// # Errors
///
/// Returns canonical R2B entry/control failures, same-driver accounting failures, transport I/O
/// failures, truncated EOF, or impossible discard-state failures.
pub fn serve_r2b_playtest_blocking_transport(
    transport: &mut TcpStream,
    session_epoch: ServerSessionEpoch,
    image: &R2bPlaytestImage,
) -> Result<R2bPlaytestExit, R2bPlaytestError> {
    let outcome = enter_r2b_play_blocking_transport(
        transport,
        session_epoch,
        &image.configuration,
        &image.bootstrap,
        selected_snapshot(&image.server_data),
    )?;
    let mut session = match outcome {
        R2bEntryOutcome::SessionClosed => return Ok(R2bPlaytestExit::SessionClosedBeforeWorld),
        R2bEntryOutcome::PeerEof => return Ok(R2bPlaytestExit::PeerEofBeforeWorld),
        R2bEntryOutcome::WorldProjectionReady(session) => session,
    };

    eprintln!(
        "R2B WorldProjectionReady | captured_play_publication=0 | pending_teleport={:?} | R2C_world_projection_pending=true",
        session
            .teleport_transaction()
            .awaiting()
            .map(|pending| pending.id)
    );
    serve_world_projection_wait(transport, &mut session)
}

fn serve_world_projection_wait(
    transport: &mut TcpStream,
    session: &mut R2bPlaySession,
) -> Result<R2bPlaytestExit, R2bPlaytestError> {
    let origin = Instant::now();
    let mut teleport_acknowledged = false;
    let mut accepted_keep_alives = 0_u64;
    let mut discarded_world_frames = 0_u64;

    loop {
        let Some(now_ms) = elapsed_millis(origin) else {
            return Ok(R2bPlaytestExit::ClockInvalid);
        };
        for _ in 0..ACTIONS_PER_SERVICE {
            match session.process_one_play_control(now_ms)? {
                R2bPlayProcess::Incomplete => break,
                R2bPlayProcess::Committed(R2bPlayInbound::TeleportAcknowledgement(result)) => {
                    if result != TeleportAckResult::Accepted {
                        return Ok(R2bPlaytestExit::InvalidTeleportAcknowledgement(result));
                    }
                    teleport_acknowledged = true;
                }
                R2bPlayProcess::Committed(R2bPlayInbound::KeepAliveAccepted { .. }) => {
                    accepted_keep_alives = accepted_keep_alives
                        .checked_add(1)
                        .ok_or(R2bPlaytestError::AccountingOverflow)?;
                }
                R2bPlayProcess::Committed(R2bPlayInbound::KeepAliveRejected { .. }) => {
                    return Ok(R2bPlaytestExit::InvalidKeepAlive);
                }
                R2bPlayProcess::Unclaimed { .. } => {
                    discard_one_complete_play_frame(session)?;
                    discarded_world_frames = discarded_world_frames
                        .checked_add(1)
                        .ok_or(R2bPlaytestError::AccountingOverflow)?;
                }
            }
        }

        write_play_once(transport, session)?;
        let Some(now_ms) = elapsed_millis(origin) else {
            return Ok(R2bPlaytestExit::ClockInvalid);
        };
        match session.service_play_liveness(now_ms)? {
            R2bLivenessProcess::Idle => {}
            R2bLivenessProcess::ChallengeQueued { .. } => write_play_once(transport, session)?,
            R2bLivenessProcess::KeepAliveTimedOut => {
                return Ok(R2bPlaytestExit::KeepAliveTimedOut);
            }
            R2bLivenessProcess::ClosedTimedOut => return Ok(R2bPlaytestExit::ClosedTimedOut),
        }

        let Some(now_ms) = elapsed_millis(origin) else {
            return Ok(R2bPlaytestExit::ClockInvalid);
        };
        let deadline_ms = session.liveness.next_deadline_ms(PLAY_LIVENESS_POLICY);
        let wait_ms = deadline_ms.saturating_sub(now_ms).max(1);
        transport
            .set_read_timeout(Some(Duration::from_millis(wait_ms)))
            .map_err(|error| transport_error("read-timeout", &error))?;

        match transport.read(&mut session.read_scratch) {
            Ok(0) => {
                if session.driver.buffered_ingress() != 0 {
                    return Err(R2bPlaytestError::TruncatedEof {
                        buffered_ingress: session.driver.buffered_ingress(),
                    });
                }
                return Ok(R2bPlaytestExit::LivePeerEof {
                    teleport_acknowledged,
                    accepted_keep_alives,
                    discarded_world_frames,
                    latency_ms: session.latency_ms(),
                });
            }
            Ok(read) => session
                .driver
                .ingest::<Infallible>(&session.read_scratch[..read])
                .map_err(R2bPlaytestError::Driver)?,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(transport_error("read", &error)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct DiscardedPlayFrame;

impl OutboundBatch for DiscardedPlayFrame {
    type Body = [u8; 0];

    fn outbound_frames(&self) -> &[Self::Body] {
        &[]
    }
}

fn discard_one_complete_play_frame(session: &mut R2bPlaySession) -> Result<(), R2bPlaytestError> {
    match session
        .driver
        .process_one_transactional::<Infallible, DiscardedPlayFrame, _>(|_| Ok(DiscardedPlayFrame))
        .map_err(R2bPlaytestError::Driver)?
    {
        TransactionResult::Committed(_) => Ok(()),
        TransactionResult::Incomplete => Err(R2bPlaytestError::DiscardInvariant),
    }
}

fn write_play_once(
    transport: &mut TcpStream,
    session: &mut R2bPlaySession,
) -> Result<(), R2bPlaytestError> {
    let pending = session.driver.pending_egress().len();
    if pending == 0 {
        return Ok(());
    }
    let result = {
        let bytes = session.driver.pending_egress();
        transport.write(bytes)
    };
    match result {
        Ok(0) => Err(R2bPlaytestError::Io {
            operation: "write",
            kind: io::ErrorKind::WriteZero,
            message: format!("zero-byte write with {pending} bytes pending"),
        }),
        Ok(written) => session
            .driver
            .consume_written::<Infallible>(written)
            .map_err(R2bPlaytestError::Driver),
        Err(error) => Err(transport_error("write", &error)),
    }
}

fn selected_snapshot(status: &ServerDataProjectionArtifact) -> FreshR2bBootstrapSnapshot<'_> {
    FreshR2bBootstrapSnapshot {
        command_key: command_key(),
        recipe_key: recipe_key(),
        login: FreshLoginPayload {
            player_id: 270,
            flags: FreshLoginFlags::SHOW_DEATH_SCREEN,
            levels: &LEVELS,
            max_players: 20,
            chunk_radius: 10,
            simulation_distance: 10,
            spawn: FreshCommonSpawnInfo {
                dimension_type_registry_id: 0,
                dimension: "minecraft:overworld",
                seed: 0x1439_77a8_ee42_e04a,
                game_mode: BootstrapGameMode::Survival,
                previous_game_mode: None,
                is_debug: false,
                is_flat: false,
                portal_cooldown: 0,
                sea_level: 63,
            },
        },
        difficulty: ChangeDifficultyPayload {
            difficulty: Difficulty26_2::Easy,
            locked: false,
        },
        abilities: PlayerAbilitiesPayload {
            flags: PlayerAbilityFlags::NONE,
            flying_speed: 0.05,
            walking_speed: 0.1,
        },
        held_slot: HeldSlotPayload(0),
        permission_event: PermissionEntityEventPayload {
            entity_id: 270,
            event: PermissionLevelEvent::All,
        },
        recipe_settings: RecipeBookSettingsPayload {
            flags: RecipeBookSettingFlags::NONE,
        },
        teleport: TeleportDestination {
            x: 10.390_952_126_751_907,
            y: 84.0,
            z: -5.815_159_807_324_014,
            yaw: f32::from_bits(0xc2ee_3335),
            pitch: f32::from_bits(0x4190_0001),
        },
        server_data: Some(ServerDataProjection {
            artifact: status,
            requested: status_key(),
        }),
        existing_players: &[],
        joining_player: SELF,
        border: WorldBorderPayload {
            center_x: 0.0,
            center_z: 0.0,
            old_size: 59_999_968.0,
            new_size: 59_999_968.0,
            lerp_time: 0,
            absolute_max_size: 29_999_984,
            warning_blocks: 5,
            warning_time: 300,
        },
        clock: ClockFullSyncPayload {
            game_time: 5_503,
            updates: &CLOCKS,
        },
        spawn: DefaultSpawnPayload {
            dimension: "minecraft:overworld",
            x: 0,
            y: 90,
            z: 0,
            yaw: 0.0,
            pitch: 0.0,
        },
        weather: BootstrapWeather::Clear,
        ticking_state: TickingStatePayload {
            tick_rate: 20.0,
            is_frozen: false,
        },
        ticking_step: TickingStepPayload(0),
        inventory: FreshEmptyInventoryPayload {
            container_id: 0,
            state_id: 1,
            slot_count: 46,
        },
    }
}

const fn revision(byte: u8) -> ProjectionRevision {
    ProjectionRevision::new([byte; 32])
}

const fn command_key() -> CommandProjectionKey {
    CommandProjectionKey::new(
        revision(1),
        revision(2),
        revision(3),
        revision(4),
        CommandPermissionProfile::DefaultNonOperator,
    )
}

const fn recipe_key() -> RecipeProjectionKey {
    RecipeProjectionKey::new(revision(5), revision(6), revision(7), revision(8))
}

const fn status_key() -> ServerDataProjectionKey {
    ServerDataProjectionKey::new(revision(9), revision(10))
}

fn read_body<R: Read>(reader: &mut R, index: usize) -> Result<Box<[u8]>, R2bPlaytestImageError> {
    let length = usize_from_u32(read_u32(reader)?)?;
    if length == 0 || length > MAX_BODY_BYTES {
        return Err(R2bPlaytestImageError::BodyLength {
            index,
            observed: length,
        });
    }
    let mut body = vec![0_u8; length].into_boxed_slice();
    reader
        .read_exact(&mut body)
        .map_err(|error| image_io_error(&error))?;
    Ok(body)
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, R2bPlaytestImageError> {
    Ok(u32::from_le_bytes(read_array(reader)?))
}

fn read_u64<R: Read>(reader: &mut R) -> Result<u64, R2bPlaytestImageError> {
    Ok(u64::from_le_bytes(read_array(reader)?))
}

fn read_array<const N: usize, R: Read>(reader: &mut R) -> Result<[u8; N], R2bPlaytestImageError> {
    let mut value = [0_u8; N];
    reader
        .read_exact(&mut value)
        .map_err(|error| image_io_error(&error))?;
    Ok(value)
}

fn usize_from_u32(value: u32) -> Result<usize, R2bPlaytestImageError> {
    usize::try_from(value).map_err(|_| R2bPlaytestImageError::BodyLength {
        index: usize::MAX,
        observed: usize::MAX,
    })
}

fn usize_from_u64(value: u64) -> Result<usize, R2bPlaytestImageError> {
    usize::try_from(value).map_err(|_| R2bPlaytestImageError::ConfigurationBytes {
        observed: usize::MAX,
    })
}

fn image_io_error(error: &io::Error) -> R2bPlaytestImageError {
    R2bPlaytestImageError::Io {
        kind: error.kind(),
        message: error.to_string(),
    }
}

fn transport_error(operation: &'static str, error: &io::Error) -> R2bPlaytestError {
    R2bPlaytestError::Io {
        operation,
        kind: error.kind(),
        message: error.to_string(),
    }
}

fn elapsed_millis(origin: Instant) -> Option<u64> {
    u64::try_from(origin.elapsed().as_millis()).ok()
}
