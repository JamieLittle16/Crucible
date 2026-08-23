use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crucible_connection_core::ConnectionLimits;
use crucible_connection_driver::{ConnectionDriver, FrameBudget, FrameFlow, StopReason};

const TIMEOUT: Duration = Duration::from_secs(5);
const RESPONSE_ID_OFFSET: u8 = 32;

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(1_024, 64 * 1_024, 64 * 1_024).expect("valid loopback limits")
}

fn one_frame() -> FrameBudget {
    FrameBudget::new(1).expect("positive frame budget")
}

fn many_frames() -> FrameBudget {
    FrameBudget::new(32).expect("positive frame budget")
}

fn body(packet_id: u8, payload: &[u8]) -> Vec<u8> {
    assert!(
        packet_id < 0x80,
        "loopback fixture uses one-byte packet IDs"
    );
    let mut body = Vec::with_capacity(payload.len() + 1);
    body.push(packet_id);
    body.extend_from_slice(payload);
    body
}

fn encode_stream(frames: &[(u8, &[u8])]) -> Vec<u8> {
    let mut driver = ConnectionDriver::new(limits());
    for &(packet_id, payload) in frames {
        driver
            .queue_frame::<()>(&body(packet_id, payload))
            .expect("fixture frame fits bounded egress");
    }
    driver.pending_egress().to_vec()
}

fn configure_socket(stream: &TcpStream) {
    stream
        .set_read_timeout(Some(TIMEOUT))
        .expect("set read timeout");
    stream
        .set_write_timeout(Some(TIMEOUT))
        .expect("set write timeout");
    stream.set_nodelay(true).expect("disable Nagle for probe");
}

fn flush_in_three_byte_chunks(stream: &mut TcpStream, driver: &mut ConnectionDriver) {
    while driver.queued_egress() != 0 {
        let written = {
            let pending = driver.pending_egress();
            let limit = pending.len().min(3);
            stream
                .write(&pending[..limit])
                .expect("bounded loopback write")
        };
        assert_ne!(written, 0, "writable loopback stream must make progress");
        driver
            .consume_written::<()>(written)
            .expect("acknowledge exact kernel write count");
    }
}

fn run_server(listener: &TcpListener, expected_frames: usize) {
    let (mut stream, _) = listener.accept().expect("accept loopback client");
    configure_socket(&stream);
    let mut driver = ConnectionDriver::new(limits());
    let mut scratch = [0_u8; 7];
    let mut committed = 0usize;

    while committed < expected_frames {
        let read = stream.read(&mut scratch).expect("bounded loopback read");
        assert_ne!(read, 0, "client closed before all frames were committed");
        driver
            .ingest::<()>(&scratch[..read])
            .expect("loopback ingress remains within bounds");

        loop {
            let mut response = None;
            let report = driver
                .process_available(one_frame(), |frame| {
                    let packet_id =
                        u8::try_from(frame.packet_id()).expect("fixture packet ID is non-negative");
                    let response_id = packet_id
                        .checked_add(RESPONSE_ID_OFFSET)
                        .expect("fixture response ID remains bounded");
                    response = Some(body(response_id, frame.payload()));
                    Ok::<_, ()>(FrameFlow::Continue)
                })
                .expect("valid loopback frame");

            if let Some(response) = response {
                assert_eq!(report.frames, 1);
                committed += 1;
                driver
                    .queue_frame::<()>(&response)
                    .expect("response remains within bounded egress");
                flush_in_three_byte_chunks(&mut stream, &mut driver);
            }

            if report.stop != StopReason::BudgetExhausted {
                break;
            }
        }
    }

    assert_eq!(driver.queued_egress(), 0);
}

#[test]
fn real_tcp_fragmentation_roundtrips_exact_borrowed_frames() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral loopback socket");
    let address = listener.local_addr().expect("read loopback address");
    let requests: [(u8, &[u8]); 5] = [
        (1, b""),
        (2, b"crucible"),
        (3, &[0, 1, 2, 3, 4, 5, 6, 7]),
        (4, b"same game. different engine."),
        (5, &[0xFF; 257]),
    ];
    let expected = requests
        .iter()
        .map(|(packet_id, payload)| {
            (
                i32::from(
                    packet_id
                        .checked_add(RESPONSE_ID_OFFSET)
                        .expect("bounded response ID"),
                ),
                payload.to_vec(),
            )
        })
        .collect::<Vec<_>>();
    let encoded = encode_stream(&requests);
    let expected_count = requests.len();

    let server = thread::spawn(move || run_server(&listener, expected_count));

    let mut client = TcpStream::connect(address).expect("connect loopback client");
    configure_socket(&client);
    for &byte in &encoded {
        client.write_all(&[byte]).expect("one-byte client write");
    }
    client
        .shutdown(Shutdown::Write)
        .expect("finish client request stream");

    let mut decoder = ConnectionDriver::new(limits());
    let mut scratch = [0_u8; 2];
    let mut observed = Vec::new();
    while observed.len() < expected.len() {
        let read = client.read(&mut scratch).expect("read loopback response");
        assert_ne!(read, 0, "server closed before all responses arrived");
        decoder
            .ingest::<()>(&scratch[..read])
            .expect("response ingress remains bounded");
        decoder
            .process_available(many_frames(), |frame| {
                observed.push((frame.packet_id(), frame.payload().to_vec()));
                Ok::<_, ()>(FrameFlow::Continue)
            })
            .expect("decode loopback responses");
    }

    assert_eq!(observed, expected);
    assert_eq!(decoder.buffered_ingress(), 0);
    server.join().expect("loopback server thread completes");
}
