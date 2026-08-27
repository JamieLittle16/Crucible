//! Product composition for Helve's first runnable Minecraft Java server slices.
//!
//! R0 composes the source-admitted Status target. R1A additionally composes the admitted offline
//! Login path up to the Configuration boundary. R1X is an explicitly experimental continuation
//! through the admitted Configuration route plus a source-free captured Play smoke prefix. R2B
//! reuses that admitted Configuration carrier with an empty captured Play image, then publishes the
//! replay-free semantic Play bootstrap through one continuing bounded Play driver. The explicit R2B
//! stock-client playtest wraps that same session only until R2C owns world projection; its compact
//! image format cannot contain captured Play publication bodies.
//! Transport composition lives here; listener/runtime policy stays in the executable, while target
//! packet semantics stay in the internal `crucible-target-26-2` crate namespace.

#![forbid(unsafe_code)]

mod login_server;
mod r1x_image;
mod r1x_server;
mod r2b_playtest;
mod r2b_server;

pub use login_server::{
    R1AConnectionExit, ServerSessionEpoch, ServerSessionEpochParseError,
    serve_r1a_blocking_transport,
};
pub use r1x_image::{R1xImageError, R1xImageSection, load_r1x_image};
pub use r1x_server::{R1xConnectionExit, serve_r1x_blocking_transport};
pub use r2b_playtest::{
    R2bPlaytestError, R2bPlaytestExit, R2bPlaytestImage, R2bPlaytestImageError,
    load_r2b_playtest_image, serve_r2b_playtest_blocking_transport,
};
pub use r2b_server::{
    R2bEntryOutcome, R2bLivenessProcess, R2bPlayError, R2bPlayInbound, R2bPlayProcess,
    R2bPlaySession, R2bServerError, enter_r2b_play_blocking_transport,
};

use std::io::{Read, Write};
use std::num::NonZeroUsize;

use crucible_connection_core::ConnectionLimits;
use crucible_preplay_io::{ActionBudget, PrePlayIo, PrePlayIoError, ServiceStop};
use crucible_target_26_2::{MAX_R0_PACKET_BODY_BYTES, Target26_2, Target26_2Error};

/// Canonical public product name.
pub const PRODUCT_NAME: &str = "Helve";
/// Canonical Minecraft server brand sent by current Helve Configuration composition.
pub const MINECRAFT_SERVER_BRAND: &str = "Helve";
/// Default localhost endpoint for the development server.
pub const DEFAULT_R0_BIND_ADDRESS: &str = "127.0.0.1:25565";
/// Sealed admission session implemented by the historical R0 composition.
pub const R0_ADMISSION_SESSION_SHA256: &str =
    "fb57c003d0e96c467dad55c209237dd23478ff287caea51943823cc62848cea0";
/// Exact status JSON used by the admitted vanilla capture and original R0 external probe.
///
/// This is historical qualification evidence and intentionally retains the old observed product
/// string. Current Helve runtime composition uses [`HELVE_STATUS_JSON`] instead.
pub const R0_ORACLE_STATUS_JSON: &str = "{\"description\":\"Crucible R0 Oracle\",\"players\":{\"max\":20,\"online\":0},\"version\":{\"name\":\"26.2\",\"protocol\":776},\"enforcesSecureChat\":true}";
/// Current product Status response for Minecraft Java 26.2 development routes.
pub const HELVE_STATUS_JSON: &str = "{\"description\":\"Helve\",\"players\":{\"max\":20,\"online\":0},\"version\":{\"name\":\"26.2\",\"protocol\":776},\"enforcesSecureChat\":true}";

const R0_FRAME_BODY_LIMIT: usize = 4 * 1_024;
const R0_INGRESS_LIMIT: usize = 16 * 1_024;
const R0_EGRESS_LIMIT: usize = 16 * 1_024;
const R0_READ_SCRATCH_BYTES: usize = 4 * 1_024;
const R0_ACTIONS_PER_SERVICE: usize = 4;
const _: () = assert!(R0_FRAME_BODY_LIMIT <= MAX_R0_PACKET_BODY_BYTES);

/// Why one blocking R0 connection finished successfully.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R0ConnectionExit {
    /// The admitted target handled a terminal action, normally Ping/Pong or duplicate status.
    SessionClosed,
    /// The peer closed cleanly after every complete inbound frame had been consumed.
    PeerEof,
}

/// Drives one blocking transport through the admitted R0 Handshake/Status protocol using Helve's
/// current product presentation.
///
/// # Errors
///
/// Returns the fail-closed bounded I/O/target error from [`PrePlayIo`].
pub fn serve_r0_blocking_transport<RW>(
    transport: &mut RW,
) -> Result<R0ConnectionExit, PrePlayIoError<Target26_2Error>>
where
    RW: Read + Write + ?Sized,
{
    serve_r0_blocking_transport_with_status(transport, HELVE_STATUS_JSON)
}

fn serve_r0_blocking_transport_with_status<RW>(
    transport: &mut RW,
    status_json: &str,
) -> Result<R0ConnectionExit, PrePlayIoError<Target26_2Error>>
where
    RW: Read + Write + ?Sized,
{
    let mut io = PrePlayIo::<Target26_2>::new(r0_limits(), read_scratch_bytes());
    let budget = action_budget();

    loop {
        let report = io.service_once(transport, status_json, budget)?;
        match report.stop {
            ServiceStop::SessionClosed => return Ok(R0ConnectionExit::SessionClosed),
            ServiceStop::PeerEof => return Ok(R0ConnectionExit::PeerEof),
            ServiceStop::InputPending
            | ServiceStop::OutputPending
            | ServiceStop::ActionBudgetExhausted => {}
        }
    }
}

fn r0_limits() -> ConnectionLimits {
    ConnectionLimits::new(R0_FRAME_BODY_LIMIT, R0_INGRESS_LIMIT, R0_EGRESS_LIMIT)
        .expect("R0 product limits are positive and coherent")
}

fn read_scratch_bytes() -> NonZeroUsize {
    NonZeroUsize::new(R0_READ_SCRATCH_BYTES).expect("R0 read scratch is positive")
}

fn action_budget() -> ActionBudget {
    ActionBudget::new(R0_ACTIONS_PER_SERVICE).expect("R0 action budget is positive")
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    use super::{
        HELVE_STATUS_JSON, R0_ORACLE_STATUS_JSON, R0ConnectionExit,
        serve_r0_blocking_transport_with_status,
    };

    #[allow(
        dead_code,
        unreachable_pub,
        reason = "the qualification fixture reuses the exact generated evidence module, while these tests consume only its admitted frame constants"
    )]
    mod admitted_codegen {
        include!("../../network/crucible-target-26-2/src/generated/status_26_2.rs");
    }

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    #[derive(Debug)]
    struct MemoryTransport {
        input: Vec<u8>,
        cursor: usize,
        read_schedule: Vec<usize>,
        read_calls: usize,
        output: Vec<u8>,
    }

    impl MemoryTransport {
        fn coalesced(input: Vec<u8>) -> Self {
            Self {
                input,
                cursor: 0,
                read_schedule: Vec::new(),
                read_calls: 0,
                output: Vec::new(),
            }
        }

        fn scheduled(input: Vec<u8>, read_schedule: Vec<usize>) -> Self {
            Self {
                read_schedule,
                ..Self::coalesced(input)
            }
        }
    }

    impl Read for MemoryTransport {
        fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
            if self.cursor == self.input.len() {
                return Ok(0);
            }
            let scheduled = self
                .read_schedule
                .get(self.read_calls)
                .copied()
                .unwrap_or(usize::MAX);
            self.read_calls = self.read_calls.saturating_add(1);
            let remaining = self.input.len() - self.cursor;
            let count = remaining.min(destination.len()).min(scheduled.max(1));
            destination[..count].copy_from_slice(&self.input[self.cursor..self.cursor + count]);
            self.cursor += count;
            Ok(count)
        }
    }

    impl Write for MemoryTransport {
        fn write(&mut self, source: &[u8]) -> io::Result<usize> {
            self.output.extend_from_slice(source);
            Ok(source.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn oracle_client_stream() -> Vec<u8> {
        let mut stream = Vec::new();
        stream.extend_from_slice(
            admitted_codegen::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_FRAME,
        );
        stream.extend_from_slice(admitted_codegen::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME);
        stream.extend_from_slice(admitted_codegen::golden::STATUS_SERVERBOUND_PING_REQUEST_FRAME);
        stream
    }

    fn oracle_server_stream() -> Vec<u8> {
        let mut stream = Vec::new();
        stream
            .extend_from_slice(admitted_codegen::golden::STATUS_CLIENTBOUND_STATUS_RESPONSE_FRAME);
        stream.extend_from_slice(admitted_codegen::golden::STATUS_CLIENTBOUND_PONG_RESPONSE_FRAME);
        stream
    }

    #[test]
    fn historical_oracle_stream_remains_byte_exact() {
        let mut transport = MemoryTransport::coalesced(oracle_client_stream());
        assert_eq!(
            serve_r0_blocking_transport_with_status(&mut transport, R0_ORACLE_STATUS_JSON),
            Ok(R0ConnectionExit::SessionClosed)
        );
        assert_eq!(transport.output, oracle_server_stream());
    }

    #[test]
    fn every_real_oracle_stream_split_point_remains_byte_equivalent() {
        let input = oracle_client_stream();
        let expected = oracle_server_stream();
        for split in 1..input.len() {
            let mut transport =
                MemoryTransport::scheduled(input.clone(), vec![split, input.len() - split]);
            assert_eq!(
                serve_r0_blocking_transport_with_status(&mut transport, R0_ORACLE_STATUS_JSON),
                Ok(R0ConnectionExit::SessionClosed),
                "split={split}"
            );
            assert_eq!(transport.output, expected, "split={split}");
        }
    }

    #[test]
    fn clean_eof_after_oracle_status_response_flushes_response_and_is_admitted() {
        let mut input = Vec::new();
        input.extend_from_slice(
            admitted_codegen::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_FRAME,
        );
        input.extend_from_slice(admitted_codegen::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME);
        let mut transport = MemoryTransport::coalesced(input);

        assert_eq!(
            serve_r0_blocking_transport_with_status(&mut transport, R0_ORACLE_STATUS_JSON),
            Ok(R0ConnectionExit::PeerEof)
        );
        assert_eq!(
            transport.output,
            admitted_codegen::golden::STATUS_CLIENTBOUND_STATUS_RESPONSE_FRAME
        );
    }

    #[test]
    fn fragmented_real_oracle_exchange_runs_through_loopback_tcp() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind loopback R0 listener");
        let address = listener.local_addr().expect("loopback listener address");
        let expected = oracle_server_stream();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept loopback R0 client");
            stream
                .set_read_timeout(Some(TEST_TIMEOUT))
                .expect("server read timeout");
            stream
                .set_write_timeout(Some(TEST_TIMEOUT))
                .expect("server write timeout");
            serve_r0_blocking_transport_with_status(&mut stream, R0_ORACLE_STATUS_JSON)
        });

        let mut client = TcpStream::connect(address).expect("connect loopback R0 client");
        client
            .set_read_timeout(Some(TEST_TIMEOUT))
            .expect("client read timeout");
        client
            .set_write_timeout(Some(TEST_TIMEOUT))
            .expect("client write timeout");
        for byte in oracle_client_stream() {
            client.write_all(&[byte]).expect("fragmented oracle write");
        }

        let mut observed = vec![0_u8; expected.len()];
        client
            .read_exact(&mut observed)
            .expect("read exact historical R0 responses");
        assert_eq!(observed, expected);
        assert_eq!(
            server.join().expect("loopback R0 server finishes"),
            Ok(R0ConnectionExit::SessionClosed)
        );
    }

    #[test]
    fn current_product_status_is_helve() {
        assert!(HELVE_STATUS_JSON.contains("\"description\":\"Helve\""));
        assert!(!HELVE_STATUS_JSON.contains("Crucible"));
    }
}
