use crucible_connection_core::{ConnectionBufferError, ConnectionLimits};
use crucible_connection_driver::{ConnectionDriver, DriverError, OutboundBatch, TransactionResult};
use crucible_session_core::{SessionPhase, SessionState};

#[derive(Debug, Eq, PartialEq)]
struct CandidateAction {
    state: SessionState,
    outbound: Vec<Vec<u8>>,
}

impl OutboundBatch for CandidateAction {
    type Body = Vec<u8>;

    fn outbound_frames(&self) -> &[Self::Body] {
        &self.outbound
    }
}

fn limits(frame_body: usize, buffer: usize) -> ConnectionLimits {
    ConnectionLimits::new(frame_body, buffer, buffer).expect("coherent qualification limits")
}

fn body(packet_id: u8, payload: &[u8]) -> Vec<u8> {
    assert!(
        packet_id < 0x80,
        "qualification helper uses one-byte packet ids"
    );
    let mut body = Vec::with_capacity(1 + payload.len());
    body.push(packet_id);
    body.extend_from_slice(payload);
    body
}

fn encoded_request(limits: ConnectionLimits) -> Vec<u8> {
    let mut encoder = ConnectionDriver::new(limits);
    encoder
        .queue_frame::<()>(&body(1, b"login"))
        .expect("qualification request fits");
    encoder.pending_egress().to_vec()
}

#[test]
fn multi_packet_capacity_failure_preserves_session_ingress_and_existing_egress() {
    let limits = limits(16, 17);
    let request = encoded_request(limits);
    let mut driver = ConnectionDriver::new(limits);
    driver
        .queue_frame::<()>(&body(90, b"old"))
        .expect("existing egress fits");
    driver.ingest::<()>(&request).expect("request ingress fits");

    let state = SessionState::new();
    let state_before = state;
    let ingress_before = driver.buffered_ingress();
    let egress_before = driver.pending_egress().to_vec();

    let error = driver
        .process_one_transactional(|frame| {
            assert_eq!(frame.packet_id(), 1);
            assert_eq!(frame.payload(), b"login");
            let mut candidate = state;
            candidate
                .advance(SessionPhase::Login)
                .expect("synthetic Login edge");
            Ok::<_, ()>(CandidateAction {
                state: candidate,
                outbound: vec![body(2, b"1234567890"), body(3, b"abcdefghij")],
            })
        })
        .expect_err("complete response batch exceeds bounded egress");

    assert!(matches!(
        error,
        DriverError::Buffer(ConnectionBufferError::EgressLimitExceeded { .. })
    ));
    assert_eq!(state, state_before);
    assert_eq!(driver.buffered_ingress(), ingress_before);
    assert_eq!(driver.pending_egress(), egress_before);
}

#[test]
fn successful_multi_packet_admission_precedes_session_adoption() {
    let limits = limits(64, 256);
    let request = encoded_request(limits);
    let mut driver = ConnectionDriver::new(limits);
    driver.ingest::<()>(&request).expect("request ingress fits");
    let mut state = SessionState::new();

    let result = driver
        .process_one_transactional(|frame| {
            assert_eq!(frame.packet_id(), 1);
            let mut candidate = state;
            candidate
                .advance(SessionPhase::Login)
                .expect("synthetic Login edge");
            Ok::<_, ()>(CandidateAction {
                state: candidate,
                outbound: vec![body(2, b"first"), body(3, b"second")],
            })
        })
        .expect("batch admission succeeds");

    let TransactionResult::Committed(action) = result else {
        panic!("complete request must commit");
    };
    assert_eq!(state.phase(), SessionPhase::Handshake);
    assert_eq!(driver.buffered_ingress(), 0);
    assert!(driver.queued_egress() > 0);

    state = action.state;
    assert_eq!(state.phase(), SessionPhase::Login);
}

#[test]
fn malformed_later_outbound_frame_rolls_back_entire_semantic_boundary() {
    let limits = limits(64, 256);
    let request = encoded_request(limits);
    let mut driver = ConnectionDriver::new(limits);
    driver
        .queue_frame::<()>(&body(90, b"existing"))
        .expect("existing egress");
    driver.ingest::<()>(&request).expect("request ingress");
    let state = SessionState::new();
    let ingress_before = driver.buffered_ingress();
    let egress_before = driver.pending_egress().to_vec();

    let error = driver
        .process_one_transactional(|_| {
            let mut candidate = state;
            candidate
                .advance(SessionPhase::Login)
                .expect("synthetic Login edge");
            Ok::<_, ()>(CandidateAction {
                state: candidate,
                outbound: vec![body(2, b"valid"), Vec::new()],
            })
        })
        .expect_err("zero-length frame body is invalid");

    assert!(matches!(
        error,
        DriverError::Buffer(ConnectionBufferError::Wire(_))
    ));
    assert_eq!(state.phase(), SessionPhase::Handshake);
    assert_eq!(driver.buffered_ingress(), ingress_before);
    assert_eq!(driver.pending_egress(), egress_before);
}
