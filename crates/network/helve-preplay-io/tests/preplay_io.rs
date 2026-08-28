use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::num::NonZeroUsize;
use std::thread;
use std::time::Duration;

use helve_connection_core::{ConnectionLimits, FrameView};
use helve_connection_driver::{ConnectionDriver, OutboundBatch};
use helve_packet_core::{PacketCodecError, PacketReader, PacketWriter};
use helve_preplay_core::{PrePlayAction, PrePlayError, PrePlayTarget};
use helve_preplay_io::{
    ActionBudget, PrePlayIo, PrePlayIoError, ProcessStop, ReadOutcome, ServiceStop, WriteOutcome,
};
use helve_session_core::{SessionPhase, SessionState, SessionStateError};

const SELECT_STATUS: i32 = 0x41;
const STATUS_QUERY: i32 = 0x42;
const CLOSE: i32 = 0x43;
const STATUS_REPLY: i32 = 0x61;
const STATUS_MAGIC: &str = "helve-io-status";
const STATUS_LABEL: &str = "helve-io-adapter";
const MAX_PACKET_BODY: usize = 512;
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Eq, PartialEq)]
enum SyntheticError {
    WrongPhase,
    Codec(PacketCodecError),
    Transition(SessionStateError),
    UnknownPacket(i32),
    InvalidMagic,
}

impl From<PacketCodecError> for SyntheticError {
    fn from(value: PacketCodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<SessionStateError> for SyntheticError {
    fn from(value: SessionStateError) -> Self {
        Self::Transition(value)
    }
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

struct SyntheticTarget;

impl PrePlayTarget for SyntheticTarget {
    type Error = SyntheticError;
    type Context = str;
    type State = ();
    type Action = Action;

    fn decode(
        context: &Self::Context,
        state: SessionState,
        _target_state: &Self::State,
        frame: FrameView<'_>,
    ) -> Result<Self::Action, Self::Error> {
        let mut reader = PacketReader::new(frame.payload());
        let mut candidate = state;
        let frames = match frame.packet_id() {
            SELECT_STATUS => {
                if state.phase() != SessionPhase::Handshake {
                    return Err(SyntheticError::WrongPhase);
                }
                if reader.read_string(64)? != STATUS_MAGIC {
                    return Err(SyntheticError::InvalidMagic);
                }
                reader.finish()?;
                candidate.advance(SessionPhase::Status)?;
                Vec::new()
            }
            STATUS_QUERY => {
                if state.phase() != SessionPhase::Status {
                    return Err(SyntheticError::WrongPhase);
                }
                let nonce = reader.read_i64()?;
                reader.finish()?;
                vec![packet_body(STATUS_REPLY, |writer| {
                    writer.write_i64(nonce)?;
                    writer.write_string(context, 64)
                })]
            }
            CLOSE => {
                reader.finish()?;
                if !candidate.close() {
                    return Err(SyntheticError::WrongPhase);
                }
                Vec::new()
            }
            packet => return Err(SyntheticError::UnknownPacket(packet)),
        };
        Ok(Action { candidate, frames })
    }
}

#[derive(Debug)]
struct MemoryTransport {
    input: Vec<u8>,
    read_cursor: usize,
    read_schedule: Vec<usize>,
    read_calls: usize,
    write_limit: usize,
    zero_write: bool,
    output: Vec<u8>,
}

impl MemoryTransport {
    fn new(input: Vec<u8>) -> Self {
        Self {
            input,
            read_cursor: 0,
            read_schedule: Vec::new(),
            read_calls: 0,
            write_limit: usize::MAX,
            zero_write: false,
            output: Vec::new(),
        }
    }

    fn scheduled(input: Vec<u8>, read_schedule: Vec<usize>) -> Self {
        Self {
            read_schedule,
            ..Self::new(input)
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
        self.read_calls = self.read_calls.saturating_add(1);
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
        if self.zero_write && !source.is_empty() {
            return Ok(0);
        }
        let count = source.len().min(self.write_limit);
        self.output.extend_from_slice(&source[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(MAX_PACKET_BODY, 256 * 1_024, 256 * 1_024).expect("coherent test limits")
}

fn scratch() -> NonZeroUsize {
    NonZeroUsize::new(17).expect("positive scratch")
}

fn large_scratch() -> NonZeroUsize {
    NonZeroUsize::new(4_096).expect("positive scratch")
}

fn budget(actions: usize) -> ActionBudget {
    ActionBudget::new(actions).expect("positive action budget")
}

fn packet_body(
    packet_id: i32,
    encode: impl FnOnce(&mut PacketWriter) -> Result<(), PacketCodecError>,
) -> Vec<u8> {
    let mut writer = PacketWriter::new(MAX_PACKET_BODY).expect("positive body bound");
    writer.write_var_int(packet_id).expect("synthetic id fits");
    encode(&mut writer).expect("synthetic payload fits");
    writer.into_bytes()
}

fn frame(body: &[u8]) -> Vec<u8> {
    let mut driver = ConnectionDriver::new(limits());
    driver.queue_frame::<()>(body).expect("frame fits");
    driver.pending_egress().to_vec()
}

fn select_status() -> Vec<u8> {
    frame(&packet_body(SELECT_STATUS, |writer| {
        writer.write_string(STATUS_MAGIC, 64)
    }))
}

fn status_query(nonce: i64) -> Vec<u8> {
    frame(&packet_body(STATUS_QUERY, |writer| writer.write_i64(nonce)))
}

fn close() -> Vec<u8> {
    frame(&packet_body(CLOSE, |_| Ok(())))
}

fn expected_status_reply(nonce: i64) -> Vec<u8> {
    frame(&packet_body(STATUS_REPLY, |writer| {
        writer.write_i64(nonce)?;
        writer.write_string(STATUS_LABEL, 64)
    }))
}

fn service_until_terminal(
    io: &mut PrePlayIo<SyntheticTarget>,
    transport: &mut MemoryTransport,
    action_budget: usize,
) -> Result<ServiceStop, PrePlayIoError<SyntheticError>> {
    for _ in 0..100_000 {
        let report = io.service_once(transport, STATUS_LABEL, budget(action_budget))?;
        match report.stop {
            ServiceStop::SessionClosed | ServiceStop::PeerEof => return Ok(report.stop),
            ServiceStop::InputPending
            | ServiceStop::OutputPending
            | ServiceStop::ActionBudgetExhausted => {}
        }
    }
    panic!("synthetic service failed to converge");
}

#[test]
fn zero_action_budget_is_unrepresentable() {
    assert_eq!(ActionBudget::new(0), None);
    assert_eq!(ActionBudget::new(1).map(ActionBudget::get), Some(1));
}

#[test]
fn every_handshake_split_point_reaches_identical_status_state() {
    let encoded = select_status();
    for split in 1..encoded.len() {
        let mut transport =
            MemoryTransport::scheduled(encoded.clone(), vec![split, encoded.len() - split]);
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());
        assert_eq!(
            service_until_terminal(&mut io, &mut transport, 8),
            Ok(ServiceStop::PeerEof)
        );
        assert_eq!(io.connection().phase(), SessionPhase::Status);
        assert_eq!(io.connection().buffered_ingress(), 0);
    }
}

#[test]
fn coalesced_actions_respect_explicit_action_budget() {
    let nonce = 0x1122_3344_5566_7788_i64;
    let mut input = select_status();
    input.extend_from_slice(&status_query(nonce));
    let mut transport = MemoryTransport::new(input);
    let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), large_scratch());

    let first = io
        .service_once(&mut transport, STATUS_LABEL, budget(1))
        .expect("first bounded service");
    assert_eq!(first.committed_actions, 1);
    assert_eq!(first.stop, ServiceStop::ActionBudgetExhausted);
    assert_eq!(io.connection().phase(), SessionPhase::Status);

    let second = io
        .service_once(&mut transport, STATUS_LABEL, budget(1))
        .expect("second bounded service");
    assert_eq!(second.committed_actions, 1);
    assert_eq!(second.outbound_frames, 1);
    assert_eq!(transport.output, expected_status_reply(nonce));
}

#[test]
fn one_byte_partial_writes_are_acknowledged_exactly() {
    let nonce = -0x0102_0304_0506_0708_i64;
    let mut input = select_status();
    input.extend_from_slice(&status_query(nonce));
    input.extend_from_slice(&close());
    let mut transport = MemoryTransport::new(input);
    transport.write_limit = 1;
    let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

    assert_eq!(
        service_until_terminal(&mut io, &mut transport, 4),
        Ok(ServiceStop::SessionClosed)
    );
    assert_eq!(transport.output, expected_status_reply(nonce));
    assert_eq!(io.connection().queued_egress(), 0);
}

#[test]
fn zero_write_fails_instead_of_spinning() {
    let nonce = 7_i64;
    let mut input = select_status();
    input.extend_from_slice(&status_query(nonce));
    let mut transport = MemoryTransport::new(input);
    transport.zero_write = true;
    let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

    let error = loop {
        if let Err(error) = io.service_once(&mut transport, STATUS_LABEL, budget(4)) {
            break error;
        }
    };
    assert!(matches!(error, PrePlayIoError::ZeroWrite { pending } if pending > 0));
    assert_eq!(io.connection().phase(), SessionPhase::Status);
    assert!(io.connection().queued_egress() > 0);
}

#[test]
fn eof_with_partial_frame_fails_closed() {
    let mut encoded = select_status();
    encoded.pop();
    let mut transport = MemoryTransport::new(encoded);
    let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

    let error = loop {
        match io.service_once(&mut transport, STATUS_LABEL, budget(4)) {
            Err(error) => break error,
            Ok(report) => assert_ne!(report.stop, ServiceStop::PeerEof),
        }
    };
    assert!(matches!(
        error,
        PrePlayIoError::TruncatedEof { buffered_ingress } if buffered_ingress > 0
    ));
    assert_eq!(io.connection().phase(), SessionPhase::Handshake);
}

#[test]
fn target_error_preserves_current_frame_and_session() {
    let malformed = frame(&packet_body(SELECT_STATUS, |writer| {
        writer.write_string("wrong", 64)
    }));
    let mut transport = MemoryTransport::new(malformed.clone());
    let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

    let error = loop {
        if let Err(error) = io.service_once(&mut transport, STATUS_LABEL, budget(4)) {
            break error;
        }
    };
    assert_eq!(
        error,
        PrePlayIoError::Connection(PrePlayError::Target(SyntheticError::InvalidMagic))
    );
    assert_eq!(io.connection().phase(), SessionPhase::Handshake);
    assert_eq!(io.connection().buffered_ingress(), malformed.len());
    assert_eq!(io.connection().queued_egress(), 0);
}

#[test]
fn terminal_close_stops_before_following_buffered_frame() {
    let mut input = select_status();
    input.extend_from_slice(&close());
    let trailing = status_query(123);
    input.extend_from_slice(&trailing);
    let mut transport = MemoryTransport::new(input);
    let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), large_scratch());

    assert!(matches!(
        io.read_once(&mut transport),
        Ok(ReadOutcome::Data(_))
    ));
    let report = io
        .process_available(STATUS_LABEL, budget(8))
        .expect("bounded buffered processing");
    assert_eq!(report.stop, ProcessStop::SessionClosed);
    assert_eq!(io.connection().phase(), SessionPhase::Closed);
    assert_eq!(io.connection().buffered_ingress(), trailing.len());
}

#[test]
fn processing_only_api_is_explicitly_bounded() {
    let mut stream = select_status();
    stream.extend_from_slice(&status_query(1));
    stream.extend_from_slice(&status_query(2));
    let mut transport = MemoryTransport::new(stream);
    let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), large_scratch());
    assert!(matches!(
        io.read_once(&mut transport),
        Ok(ReadOutcome::Data(_))
    ));

    let report = io
        .process_available(STATUS_LABEL, budget(2))
        .expect("bounded processing");
    assert_eq!(report.committed_actions, 2);
    assert_eq!(report.stop, ProcessStop::ActionBudgetExhausted);
    assert!(io.connection().buffered_ingress() > 0);
}

#[test]
fn ten_thousand_status_actions_reuse_one_retained_adapter() {
    const QUERIES: usize = 10_000;
    let mut input = select_status();
    let mut expected = Vec::new();
    for index in 0..QUERIES {
        let nonce = i64::try_from(index).expect("bounded test nonce");
        input.extend_from_slice(&status_query(nonce));
        expected.extend_from_slice(&expected_status_reply(nonce));
    }
    input.extend_from_slice(&close());
    let mut transport = MemoryTransport::new(input);
    transport.read_schedule = vec![3; 50_000];
    transport.write_limit = 11;
    let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());

    assert_eq!(
        service_until_terminal(&mut io, &mut transport, 7),
        Ok(ServiceStop::SessionClosed)
    );
    assert_eq!(transport.output, expected);
    assert_eq!(io.connection().buffered_ingress(), 0);
    assert_eq!(io.connection().queued_egress(), 0);
}

#[test]
fn synthetic_target_runs_through_real_loopback_tcp() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback");
    let address = listener.local_addr().expect("loopback address");
    let nonce = 0x0102_0304_0506_0708_i64;
    let mut request = select_status();
    request.extend_from_slice(&status_query(nonce));
    request.extend_from_slice(&close());
    let expected = expected_status_reply(nonce);

    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept loopback client");
        stream
            .set_read_timeout(Some(TIMEOUT))
            .expect("server read timeout");
        stream
            .set_write_timeout(Some(TIMEOUT))
            .expect("server write timeout");
        let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), scratch());
        loop {
            let report = io
                .service_once(&mut stream, STATUS_LABEL, budget(4))
                .expect("loopback service");
            if report.stop == ServiceStop::SessionClosed {
                break;
            }
        }
    });

    let mut client = TcpStream::connect(address).expect("connect loopback");
    client
        .set_read_timeout(Some(TIMEOUT))
        .expect("client read timeout");
    client
        .set_write_timeout(Some(TIMEOUT))
        .expect("client write timeout");
    for byte in request {
        client.write_all(&[byte]).expect("fragmented client write");
    }
    let mut observed = vec![0_u8; expected.len()];
    client
        .read_exact(&mut observed)
        .expect("read exact status reply");
    assert_eq!(observed, expected);
    server.join().expect("loopback server finishes");
}

#[test]
fn write_once_exposes_partial_progress_without_copying_to_second_queue() {
    let nonce = 99_i64;
    let mut input = select_status();
    input.extend_from_slice(&status_query(nonce));
    let mut transport = MemoryTransport::new(input);
    let mut io = PrePlayIo::<SyntheticTarget>::new(limits(), large_scratch());

    assert!(matches!(
        io.read_once(&mut transport),
        Ok(ReadOutcome::Data(_))
    ));
    let process = io
        .process_available(STATUS_LABEL, budget(2))
        .expect("queue status response");
    assert_eq!(process.committed_actions, 2);
    assert_eq!(process.outbound_frames, 1);

    let before = io.connection().queued_egress();
    assert!(before > 2);
    transport.write_limit = 2;
    assert_eq!(
        io.write_once(&mut transport),
        Ok(WriteOutcome::Progress {
            written: 2,
            remaining: before - 2,
        })
    );
}
