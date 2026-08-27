//! Replay-free R2B Configuration -> Play entry composition.
//!
//! This development composition intentionally reuses the source-admitted R1X Configuration target
//! with an empty captured Play image, then destroys the fully drained pre-play driver and creates
//! exactly one Play driver. That Play driver publishes the canonical [`PreparedR2bPlan`] and is
//! returned intact at the explicit `WorldProjection` seam so R2C/live Play can continue without a
//! second queue or another driver allocation.
//!
//! Captured Play bodies are rejected before transport I/O. R1X is used only as the temporary
//! Configuration carrier; every non-world Play body comes from replay-free R2B semantic projection.

use std::convert::Infallible;
use std::io::{self, Read, Write};
use std::num::NonZeroUsize;

use crucible_connection_core::ConnectionLimits;
use crucible_connection_driver::{ConnectionDriver, DriverError};
use crucible_packet_core::PacketWriter;
use crucible_preplay_core::{PrePlayConnection, PrePlayError};
use crucible_preplay_io::{
    ActionBudget, IoOperation, PrePlayIo, PrePlayIoError, PublicationServiceStop,
};
use crucible_publication_core::{
    StagedPublicationCursor, StagedPublicationStep, publish_staged_plan_one,
};
use crucible_session_core::SessionPhase;
use crucible_target_26_2::{
    R1xError, Target26_2R1x, Target26_2R1xContext, Target26_2R1xState,
    r2b::{
        FreshR2bBootstrapSnapshot, PLAY_PUBLICATION_STAGES, PlayBootstrapImage26_2,
        PlayBootstrapStage, PlayPacketIds, PrepareR2bError, PreparedR2bPlan,
        SELECTED_DYNAMIC_ARENA_CAPACITY, TeleportTransaction, stage_for_publication_index,
    },
};

use crate::ServerSessionEpoch;

const FRAME_BODY_LIMIT: usize = 65_536;
const INGRESS_LIMIT: usize = 256 * 1_024;
const EGRESS_LIMIT: usize = 128 * 1_024;
const READ_SCRATCH_BYTES: usize = 16 * 1_024;
const PREPLAY_ACTIONS_PER_SERVICE: usize = 4;
const PREPARE_SCRATCH_BYTES: usize = 4 * 1_024;
const _: () = assert!(PREPLAY_ACTIONS_PER_SERVICE > 0);

/// Fail-closed R2B server-composition error.
#[derive(Debug)]
pub enum R2bServerError {
    /// The supplied R1X context still contains captured Play bodies.
    CapturedPlayNotEmpty {
        /// Captured body count that would otherwise be replayed.
        frames: usize,
        /// Aggregate captured Play body bytes.
        body_bytes: usize,
    },
    /// Source-admitted pre-play I/O/target failure.
    PrePlay(PrePlayIoError<R1xError>),
    /// Replay-free semantic bootstrap preparation failed.
    Prepare(PrepareR2bError),
    /// The finite staged publication did not reach its explicit complete observation within the
    /// exact body-plus-stage action bound.
    PublicationDidNotConverge,
}

impl From<PrePlayIoError<R1xError>> for R2bServerError {
    fn from(value: PrePlayIoError<R1xError>) -> Self {
        Self::PrePlay(value)
    }
}

impl From<PrepareR2bError> for R2bServerError {
    fn from(value: PrepareR2bError) -> Self {
        Self::Prepare(value)
    }
}

/// Connection-owned state handed to the world/live-Play owner after R2B network bootstrap.
///
/// The driver is the same single bounded Play driver that admitted the R2B frames. Its egress is
/// fully drained before handoff and R2B never reads Play ingress, so no userspace packet bytes are
/// discarded at this boundary. The teleport transaction remains pending until the client sends the
/// exact acknowledgement required by `SEM-NET-R2B-PLAY-006`.
pub struct R2bPlaySession {
    pub(crate) driver: ConnectionDriver,
    pub(crate) teleport: TeleportTransaction,
}

impl R2bPlaySession {
    /// Outstanding teleport transaction carried into continuing Play processing.
    #[must_use]
    pub const fn teleport_transaction(&self) -> TeleportTransaction {
        self.teleport
    }

    /// Userspace ingress bytes already buffered in the continuing Play driver.
    #[must_use]
    pub fn buffered_ingress(&self) -> usize {
        self.driver.buffered_ingress()
    }

    /// Userspace egress bytes still queued in the continuing Play driver.
    #[must_use]
    pub fn queued_egress(&self) -> usize {
        self.driver.queued_egress()
    }
}

impl core::fmt::Debug for R2bPlaySession {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("R2bPlaySession")
            .field("buffered_ingress", &self.driver.buffered_ingress())
            .field("queued_egress", &self.driver.queued_egress())
            .field("teleport", &self.teleport)
            .finish()
    }
}

/// Result of attempting the R2B entry boundary on one connection.
#[derive(Debug)]
pub enum R2bEntryOutcome {
    /// The target committed a terminal session before reaching Play and all queued output drained.
    SessionClosed,
    /// The peer closed cleanly before the replay-free Play handoff.
    PeerEof,
    /// R2B network bootstrap is flushed and ownership is ready to pass to R2C/live Play.
    WorldProjectionReady(R2bPlaySession),
}

/// Drives one transport through Login, source-admitted Configuration, zero captured Play bodies and
/// the complete replay-free R2B network bootstrap.
///
/// The pre-play object is dropped only after the generic phase is Play, empty-Play publication is
/// committed, and both of its userspace queues are empty. One fresh bounded Play driver is then
/// created, receives the prepared R2B plan through [`publish_staged_plan_one`], flushes that egress,
/// and is returned rather than replaced. This makes the explicit `WorldProjection` seam a real
/// ownership boundary with one continuing queue.
///
/// # Errors
///
/// Fails before I/O when captured Play is non-empty, then preserves the existing fail-closed
/// pre-play/driver errors and transactional R2B preparation errors.
pub fn enter_r2b_play_blocking_transport<RW>(
    transport: &mut RW,
    session_epoch: ServerSessionEpoch,
    preplay_context: &Target26_2R1xContext,
    bootstrap_image: &PlayBootstrapImage26_2,
    snapshot: FreshR2bBootstrapSnapshot<'_>,
    packet_ids: PlayPacketIds,
) -> Result<R2bEntryOutcome, R2bServerError>
where
    RW: Read + Write + ?Sized,
{
    if preplay_context.play_frame_count() != 0 {
        return Err(R2bServerError::CapturedPlayNotEmpty {
            frames: preplay_context.play_frame_count(),
            body_bytes: preplay_context.play_body_bytes(),
        });
    }

    let target_state = Target26_2R1xState::with_login_session_uuid(session_epoch.into_bytes());
    let connection = PrePlayConnection::<Target26_2R1x>::with_target_state(limits(), target_state);
    let mut io = PrePlayIo::from_connection(connection, read_scratch_bytes());
    let budget = action_budget();

    loop {
        if ready_for_r2b_handoff(&io) {
            break;
        }

        let report = io.service_once_with_publication(transport, preplay_context, budget)?;
        match report.stop {
            PublicationServiceStop::SessionClosed => return Ok(R2bEntryOutcome::SessionClosed),
            PublicationServiceStop::PeerEof => return Ok(R2bEntryOutcome::PeerEof),
            PublicationServiceStop::InputPending
            | PublicationServiceStop::OutputPending
            | PublicationServiceStop::ActionBudgetExhausted
            | PublicationServiceStop::PublicationProgress => {}
        }
    }

    debug_assert!(ready_for_r2b_handoff(&io));
    drop(io);

    let mut scratch = PacketWriter::new(PREPARE_SCRATCH_BYTES)
        .map_err(PrepareR2bError::from)
        .map_err(R2bServerError::from)?;
    let mut teleport = TeleportTransaction::new();
    let plan = PreparedR2bPlan::prepare(
        snapshot,
        bootstrap_image,
        packet_ids,
        &mut scratch,
        &mut teleport,
        SELECTED_DYNAMIC_ARENA_CAPACITY,
    )?;
    debug_assert!(scratch.is_empty());

    let mut driver = ConnectionDriver::new(limits());
    publish_complete_plan(&plan, &mut driver)?;
    drain_egress(transport, &mut driver)?;

    debug_assert_eq!(driver.buffered_ingress(), 0);
    debug_assert_eq!(driver.queued_egress(), 0);
    Ok(R2bEntryOutcome::WorldProjectionReady(R2bPlaySession {
        driver,
        teleport,
    }))
}

fn ready_for_r2b_handoff(io: &PrePlayIo<Target26_2R1x>) -> bool {
    let connection = io.connection();
    connection.phase() == SessionPhase::Play
        && connection.target_state().replay_complete()
        && connection.buffered_ingress() == 0
        && connection.queued_egress() == 0
}

fn publish_complete_plan(
    plan: &PreparedR2bPlan<'_>,
    driver: &mut ConnectionDriver,
) -> Result<(), R2bServerError> {
    let action_bound = plan
        .body_count()
        .checked_add(PLAY_PUBLICATION_STAGES.len())
        .and_then(|value| value.checked_add(1))
        .ok_or(R2bServerError::PublicationDidNotConverge)?;
    let mut cursor = StagedPublicationCursor::new();

    for _ in 0..action_bound {
        match publish_staged_plan_one::<Infallible, _>(plan, &mut cursor, driver)
            .map_err(|error| R2bServerError::PrePlay(map_driver_error(&error)))?
        {
            StagedPublicationStep::Complete => {
                debug_assert_eq!(
                    stage_for_publication_index(cursor.stage_index()),
                    PlayBootstrapStage::WorldProjection
                );
                return Ok(());
            }
            StagedPublicationStep::StageComplete { .. }
            | StagedPublicationStep::Queued { .. } => {}
        }
    }

    Err(R2bServerError::PublicationDidNotConverge)
}

fn drain_egress<RW>(
    transport: &mut RW,
    driver: &mut ConnectionDriver,
) -> Result<(), R2bServerError>
where
    RW: Write + ?Sized,
{
    while driver.queued_egress() != 0 {
        let pending = driver.pending_egress().len();
        let write_result = {
            let bytes = driver.pending_egress();
            transport.write(bytes)
        };
        match write_result {
            Ok(0) => {
                return Err(R2bServerError::PrePlay(PrePlayIoError::ZeroWrite {
                    pending,
                }));
            }
            Ok(written) => driver
                .consume_written::<Infallible>(written)
                .map_err(|error| R2bServerError::PrePlay(map_driver_error(&error)))?,
            Err(error) => {
                return Err(R2bServerError::PrePlay(io_error(
                    IoOperation::Write,
                    &error,
                )));
            }
        }
    }
    Ok(())
}

fn map_driver_error(error: &DriverError<Infallible>) -> PrePlayIoError<R1xError> {
    match error {
        DriverError::Buffer(error) => PrePlayIoError::Connection(PrePlayError::Buffer(*error)),
        DriverError::Handler(never) => match *never {},
        DriverError::RollbackFailed {
            operation,
            rollback,
        } => PrePlayIoError::Connection(PrePlayError::RollbackFailed {
            operation: *operation,
            rollback: *rollback,
        }),
        DriverError::AccountingOverflow => PrePlayIoError::AccountingOverflow,
    }
}

fn io_error(operation: IoOperation, error: &io::Error) -> PrePlayIoError<R1xError> {
    PrePlayIoError::Io {
        operation,
        kind: error.kind(),
        message: error.to_string(),
    }
}

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(FRAME_BODY_LIMIT, INGRESS_LIMIT, EGRESS_LIMIT)
        .expect("R2B development limits are positive and coherent")
}

fn read_scratch_bytes() -> NonZeroUsize {
    NonZeroUsize::new(READ_SCRATCH_BYTES).expect("R2B read scratch is positive")
}

fn action_budget() -> ActionBudget {
    ActionBudget::new(PREPLAY_ACTIONS_PER_SERVICE).expect("R2B action budget is positive")
}

#[cfg(test)]
mod tests {
    use crucible_connection_driver::ConnectionDriver;
    use crucible_packet_core::PacketWriter;
    use crucible_publication_core::StagedPublicationPlan;
    use crucible_target_26_2::r2b::{
        CommandPermissionProfile, CommandProjectionKey, PlayBootstrapImage26_2,
        ProjectionRevision, QualifiedProjectionArtifact, RecipeProjectionKey,
    };

    use super::{EGRESS_LIMIT, FRAME_BODY_LIMIT, INGRESS_LIMIT, limits};

    const fn revision(byte: u8) -> ProjectionRevision {
        ProjectionRevision::new([byte; 32])
    }

    fn assert_plan_trait<T: StagedPublicationPlan>() {}

    #[test]
    fn r2b_entry_limits_are_finite_and_share_the_existing_single_driver_shape() {
        let limits = limits();
        assert_eq!(limits.max_frame_body_len(), FRAME_BODY_LIMIT);
        assert_eq!(limits.max_ingress_buffered(), INGRESS_LIMIT);
        assert_eq!(limits.max_egress_queued(), EGRESS_LIMIT);
        let driver = ConnectionDriver::new(limits);
        assert_eq!(driver.buffered_ingress(), 0);
        assert_eq!(driver.queued_egress(), 0);
    }

    #[test]
    fn shared_image_construction_remains_process_owned_not_driver_owned() {
        let command_key = CommandProjectionKey::new(
            revision(1),
            revision(2),
            revision(3),
            revision(4),
            CommandPermissionProfile::DefaultNonOperator,
        );
        let recipe_key = RecipeProjectionKey::new(
            revision(5),
            revision(6),
            revision(7),
            revision(8),
        );
        let image = PlayBootstrapImage26_2::new(
            QualifiedProjectionArtifact::new(command_key, vec![16, 0].into_boxed_slice())
                .expect("command artifact"),
            QualifiedProjectionArtifact::new(
                recipe_key,
                vec![0x85, 0x01, 0].into_boxed_slice(),
            )
            .expect("recipe artifact"),
        );

        let mut scratch = PacketWriter::new(8).expect("bounded scratch");
        scratch.write_u8(1).expect("test byte");
        assert_eq!(scratch.as_slice(), &[1]);
        assert_eq!(image.commands(&command_key), Ok(&[16, 0][..]));
        assert_eq!(
            image.update_recipes(&recipe_key),
            Ok(&[0x85, 0x01, 0][..])
        );

        assert_plan_trait::<crucible_target_26_2::r2b::PreparedR2bPlan<'_>>();
    }
}
