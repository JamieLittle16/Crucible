use std::io::{self, Read, Write};
use std::num::NonZeroUsize;

use crucible_connection_core::{ConnectionLimits, FrameView};
use crucible_connection_driver::{ConnectionDriver, OutboundBatch};
use crucible_preplay_core::{
    PrePlayAction, PrePlayPublication, PrePlayPublisher, PrePlayTarget, PublicationCursor,
    PublicationStep,
};
use crucible_preplay_io::{ActionBudget, PrePlayIo, PublicationServiceStop};
use crucible_session_core::{SessionPhase, SessionState};

const SELECT_LOGIN: i32 = 0x21;
const LOGIN_ACK: i32 = 0x22;
const CONFIG_INPUT: i32 = 0x23;
const MAX_PACKET_BODY: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SyntheticError {
    WrongPhase,
    UnknownPacket(i32),
    TrailingPayload,
}

#[derive(Debug)]
struct Action {
    candidate: SessionState,
    frames: Vec<Vec<u8>>,
}

impl OutboundBatch for Action {
    type Body = Vec<u8>;

    fn outbound_frames(&self) -> &[Self::Body] {
        &self.frames
    }
}

impl PrePlayAction for Action {
    fn candidate_session(&self) -> SessionState {
        self.candidate
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct PublishingState {
    cursor: PublicationCursor,
    commits: usize,
    done: bool,
}

#[derive(Debug)]
struct PublishingContext {
    bodies: Vec<Vec<u8>>,
}

struct PublishingTarget;

type Proposal<'a> = Result<Option<PrePlayPublication<'a, Vec<u8>, usize>>, SyntheticError>;

impl PrePlayTarget for PublishingTarget {
    type Error = SyntheticError;
    type Context = PublishingContext;
    type State = PublishingState;
    type Action = Action;

    fn decode(
        _context: &Self::Context,
        state: SessionState,
        _target_state: &Self::State,
        frame: FrameView<'_>,
    ) -> Result<Self::Action, Self::Error> {
        if !frame.payload().is_empty() {
            return Err(SyntheticError::TrailingPayload);
        }

        let mut candidate = state;
        match frame.packet_id() {
            SELECT_LOGIN if state.phase() == SessionPhase::Handshake => candidate
                .advance(SessionPhase::Login)
                .map_err(|_| SyntheticError::WrongPhase)?,
            LOGIN_ACK if state.phase() == SessionPhase::Login => candidate
                .advance(SessionPhase::Configuration)
                .map_err(|_| SyntheticError::WrongPhase)?,
            CONFIG_INPUT if state.phase() == SessionPhase::Configuration => {}
            SELECT_LOGIN | LOGIN_ACK | CONFIG_INPUT => return Err(SyntheticError::WrongPhase),
            packet => return Err(SyntheticError::UnknownPacket(packet)),
        }

        Ok(Action {
            candidate,
            frames: Vec::new(),
        })
    }
}

impl PrePlayPublisher for PublishingTarget {
    type PublicationBody = Vec<u8>;
    type PublicationCommit = usize;

    fn publication<'a>(
        context: &'a Self::Context,
        session: SessionState,
        target_state: &'a Self::State,
    ) -> Proposal<'a> {
        if session.phase() != SessionPhase::Configuration || target_state.done {
            return Ok(None);
        }
        Ok(Some(PrePlayPublication::new(
            &context.bodies,
            target_state.cursor,
            target_state.commits + 1,
        )))
    }

    fn commit_publication(
        state: &mut Self::State,
        commit: Self::PublicationCommit,
        cursor: PublicationCursor,
        step: PublicationStep,
    ) {
        state.cursor = cursor;
        state.commits = commit;
        if step == PublicationStep::Complete {
            state.done = true;
        }
    }
}

#[derive(Debug)]
struct MemoryTransport {
    input: Vec<u8>,
    read_cursor: usize,
    read_schedule: Vec<usize>,
    read_calls: usize,
    write_limit: usize,
    output: Vec<u8>,
}

impl MemoryTransport {
    fn scheduled(input: Vec<u8>, read_schedule: Vec<usize>) -> Self {
        Self {
            input,
            read_cursor: 0,
            read_schedule,
            read_calls: 0,
            write_limit: usize::MAX,
            output: Vec::new(),
        }
    }
}

impl Read for MemoryTransport {
    fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
        if self.read_cursor == self.input.len() {
            return Ok(0);
        }
        let scheduled = self
            .read_schedule
            .get(self.read_calls)
            .copied()
            .unwrap_or(usize::MAX);
        self.read_calls += 1;
        let available = self.input.len() - self.read_cursor;
        let count = available.min(destination.len()).min(scheduled.max(1));
        destination[..count]
            .copy_from_slice(&self.input[self.read_cursor..self.read_cursor + count]);
        self.read_cursor += count;
        Ok(count)
    }
}

impl Write for MemoryTransport {
    fn write(&mut self, source: &[u8]) -> io::Result<usize> {
        let count = source.len().min(self.write_limit);
        self.output.extend_from_slice(&source[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(MAX_PACKET_BODY, 4 * 1_024, 4 * 1_024).expect("coherent I/O limits")
}

fn scratch() -> NonZeroUsize {
    NonZeroUsize::new(1_024).expect("positive scratch")
}

fn budget(actions: usize) -> ActionBudget {
    ActionBudget::new(actions).expect("positive action budget")
}

fn frame(packet_id: i32) -> Vec<u8> {
    let body = [u8::try_from(packet_id).expect("single-byte synthetic VarInt")];
    encoded_body(&body)
}

fn encoded_body(body: &[u8]) -> Vec<u8> {
    let mut driver = ConnectionDriver::new(limits());
    driver.queue_frame::<()>(body).expect("synthetic body fits");
    driver.pending_egress().to_vec()
}

fn context() -> PublishingContext {
    PublishingContext {
        bodies: vec![vec![0x71, 0xAA], vec![0x72, 0xBB, 0xCC]],
    }
}

fn configuration_prefix() -> Vec<u8> {
    let mut input = frame(SELECT_LOGIN);
    input.extend_from_slice(&frame(LOGIN_ACK));
    input
}

fn io() -> PrePlayIo<PublishingTarget> {
    PrePlayIo::new(limits(), scratch())
}

#[test]
fn newly_entered_configuration_publishes_before_a_second_read() {
    let prefix = configuration_prefix();
    let mut input = prefix.clone();
    input.extend_from_slice(&frame(CONFIG_INPUT));
    let mut transport = MemoryTransport::scheduled(input, vec![prefix.len()]);
    let mut io = io();
    let context = context();

    let report = io
        .service_once_with_publication(&mut transport, &context, budget(8))
        .expect("publication-aware service");

    assert_eq!(report.read_bytes, prefix.len());
    assert_eq!(report.committed_actions, 3);
    assert_eq!(report.outbound_frames, 1);
    assert_eq!(report.stop, PublicationServiceStop::PublicationProgress);
    assert_eq!(transport.read_calls, 1);
    assert_eq!(transport.read_cursor, prefix.len());
    assert_eq!(transport.output, encoded_body(&context.bodies[0]));
    assert_eq!(io.connection().phase(), SessionPhase::Configuration);
    assert_eq!(io.connection().target_state().cursor.next_index(), 1);
}

#[test]
fn large_budget_still_allows_only_one_proactive_step_per_service_call() {
    let prefix = configuration_prefix();
    let mut input = prefix.clone();
    input.extend_from_slice(&frame(CONFIG_INPUT));
    let mut transport = MemoryTransport::scheduled(input, vec![prefix.len()]);
    let mut io = io();
    let context = context();

    let first = io
        .service_once_with_publication(&mut transport, &context, budget(8))
        .expect("first publication service");
    assert_eq!(first.outbound_frames, 1);
    assert_eq!(io.connection().target_state().cursor.next_index(), 1);

    let second = io
        .service_once_with_publication(&mut transport, &context, budget(8))
        .expect("second publication service");
    assert_eq!(second.read_bytes, 0);
    assert_eq!(second.committed_actions, 1);
    assert_eq!(second.outbound_frames, 1);
    assert_eq!(second.stop, PublicationServiceStop::PublicationProgress);
    assert_eq!(transport.read_calls, 1);
    assert_eq!(io.connection().target_state().cursor.next_index(), 2);

    let mut expected = encoded_body(&context.bodies[0]);
    expected.extend_from_slice(&encoded_body(&context.bodies[1]));
    assert_eq!(transport.output, expected);

    let complete = io
        .service_once_with_publication(&mut transport, &context, budget(8))
        .expect("completion service");
    assert_eq!(complete.read_bytes, 0);
    assert_eq!(complete.committed_actions, 1);
    assert_eq!(complete.outbound_frames, 0);
    assert_eq!(complete.stop, PublicationServiceStop::PublicationProgress);
    assert_eq!(transport.read_calls, 1);
    assert!(io.connection().target_state().done);

    let inbound = io
        .service_once_with_publication(&mut transport, &context, budget(8))
        .expect("post-publication inbound service");
    assert_eq!(inbound.read_bytes, frame(CONFIG_INPUT).len());
    assert_eq!(inbound.committed_actions, 1);
    assert_eq!(inbound.outbound_frames, 0);
    assert_eq!(transport.read_calls, 2);
}

#[test]
fn inbound_budget_exhaustion_defers_publication() {
    let prefix = configuration_prefix();
    let mut transport = MemoryTransport::scheduled(prefix, Vec::new());
    let mut io = io();
    let context = context();

    let inbound = io
        .service_once_with_publication(&mut transport, &context, budget(2))
        .expect("two inbound transitions");
    assert_eq!(inbound.committed_actions, 2);
    assert_eq!(inbound.outbound_frames, 0);
    assert_eq!(inbound.stop, PublicationServiceStop::ActionBudgetExhausted);
    assert_eq!(io.connection().phase(), SessionPhase::Configuration);
    assert_eq!(io.connection().target_state().cursor.next_index(), 0);
    assert!(transport.output.is_empty());

    let publication = io
        .service_once_with_publication(&mut transport, &context, budget(1))
        .expect("publication gets next budget unit");
    assert_eq!(publication.committed_actions, 1);
    assert_eq!(publication.outbound_frames, 1);
    assert_eq!(
        publication.stop,
        PublicationServiceStop::ActionBudgetExhausted
    );
    assert_eq!(io.connection().target_state().cursor.next_index(), 1);
}

#[test]
fn zero_byte_complete_step_consumes_budget_and_cannot_spin() {
    let mut transport = MemoryTransport::scheduled(configuration_prefix(), Vec::new());
    let mut io = io();
    let context = context();

    let enter = io
        .service_once_with_publication(&mut transport, &context, budget(2))
        .expect("enter configuration without spare budget");
    assert_eq!(enter.stop, PublicationServiceStop::ActionBudgetExhausted);

    for expected_cursor in 1..=2 {
        let report = io
            .service_once_with_publication(&mut transport, &context, budget(1))
            .expect("one body per one-action turn");
        assert_eq!(report.committed_actions, 1);
        assert_eq!(report.outbound_frames, 1);
        assert_eq!(report.stop, PublicationServiceStop::ActionBudgetExhausted);
        assert_eq!(
            io.connection().target_state().cursor.next_index(),
            expected_cursor
        );
    }

    let before_bytes = transport.output.len();
    let complete = io
        .service_once_with_publication(&mut transport, &context, budget(1))
        .expect("zero-byte completion consumes one turn");
    assert_eq!(complete.committed_actions, 1);
    assert_eq!(complete.outbound_frames, 0);
    assert_eq!(complete.stop, PublicationServiceStop::ActionBudgetExhausted);
    assert_eq!(transport.output.len(), before_bytes);
    assert!(io.connection().target_state().done);
}

#[test]
fn output_backpressure_blocks_another_publication_commit() {
    let mut transport = MemoryTransport::scheduled(configuration_prefix(), Vec::new());
    transport.write_limit = 1;
    let mut io = io();
    let context = context();

    let first = io
        .service_once_with_publication(&mut transport, &context, budget(8))
        .expect("first body commits before partial write");
    assert_eq!(first.outbound_frames, 1);
    assert_eq!(first.stop, PublicationServiceStop::OutputPending);
    assert_eq!(io.connection().target_state().cursor.next_index(), 1);
    let state_after_queue = *io.connection().target_state();

    let second = io
        .service_once_with_publication(&mut transport, &context, budget(8))
        .expect("pending egress gets write priority");
    assert_eq!(second.committed_actions, 0);
    assert_eq!(second.outbound_frames, 0);
    assert_eq!(second.stop, PublicationServiceStop::OutputPending);
    assert_eq!(*io.connection().target_state(), state_after_queue);
}
