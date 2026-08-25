//! Product composition for the experimental R1X Configuration -> Play smoke route.
//!
//! This extends the same target-neutral connection/I/O stack used by R1A. Configuration and the
//! selected captured Play prefix drain through `PrePlayPublisher`; there is no replay-specific
//! socket path, second egress queue or unbounded publication loop.

use std::io::{Read, Write};
use std::num::NonZeroUsize;

use crucible_connection_core::ConnectionLimits;
use crucible_preplay_core::PrePlayConnection;
use crucible_preplay_io::{ActionBudget, PrePlayIo, PrePlayIoError, PublicationServiceStop};
use crucible_target_26_2::{R1xError, Target26_2R1x, Target26_2R1xContext, Target26_2R1xState};

use crate::ServerSessionEpoch;

const FRAME_BODY_LIMIT: usize = 65_536;
const INGRESS_LIMIT: usize = 256 * 1_024;
const EGRESS_LIMIT: usize = 128 * 1_024;
const READ_SCRATCH_BYTES: usize = 16 * 1_024;
const ACTIONS_PER_SERVICE: usize = 4;

/// Why one R1X development connection ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R1xConnectionExit {
    /// The target committed a terminal session state and all queued output was flushed.
    SessionClosed,
    /// The peer closed cleanly after all complete inbound frames were consumed.
    PeerEof,
}

/// Drives one blocking transport through Login, real Configuration, and the selected R1X Play
/// replay prefix.
///
/// After the replay prefix completes this function deliberately keeps the socket alive and continues
/// consuming bounded experimental Play input until the peer closes. Returning immediately would
/// turn successful replay publication into an artificial server disconnect exactly when the client
/// reaches the state we are trying to inspect.
///
/// # Errors
///
/// Returns the existing fail-closed bounded I/O/target error. No R1X error path bypasses transactional
/// ingress/egress or publication state adoption.
pub fn serve_r1x_blocking_transport<RW>(
    transport: &mut RW,
    session_epoch: ServerSessionEpoch,
    context: &Target26_2R1xContext,
) -> Result<R1xConnectionExit, PrePlayIoError<R1xError>>
where
    RW: Read + Write + ?Sized,
{
    let target_state = Target26_2R1xState::with_login_session_uuid(session_epoch.into_bytes());
    let connection = PrePlayConnection::<Target26_2R1x>::with_target_state(limits(), target_state);
    let mut io = PrePlayIo::from_connection(connection, read_scratch_bytes());
    let budget = action_budget();

    loop {
        let report = io.service_once_with_publication(transport, context, budget)?;
        match report.stop {
            PublicationServiceStop::SessionClosed => return Ok(R1xConnectionExit::SessionClosed),
            PublicationServiceStop::PeerEof => return Ok(R1xConnectionExit::PeerEof),
            PublicationServiceStop::InputPending
            | PublicationServiceStop::OutputPending
            | PublicationServiceStop::ActionBudgetExhausted
            | PublicationServiceStop::PublicationProgress => {}
        }
    }
}

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(FRAME_BODY_LIMIT, INGRESS_LIMIT, EGRESS_LIMIT)
        .expect("R1X product limits are positive and coherent")
}

fn read_scratch_bytes() -> NonZeroUsize {
    NonZeroUsize::new(READ_SCRATCH_BYTES).expect("R1X read scratch is positive")
}

fn action_budget() -> ActionBudget {
    ActionBudget::new(ACTIONS_PER_SERVICE).expect("R1X action budget is positive")
}

#[cfg(test)]
mod tests {
    use super::{EGRESS_LIMIT, FRAME_BODY_LIMIT, INGRESS_LIMIT, limits};

    #[test]
    fn r1x_limits_are_finite_and_cover_one_maximal_body() {
        let limits = limits();
        assert_eq!(limits.max_frame_body_len(), FRAME_BODY_LIMIT);
        assert_eq!(limits.max_ingress_buffered(), INGRESS_LIMIT);
        assert_eq!(limits.max_egress_queued(), EGRESS_LIMIT);
    }
}
