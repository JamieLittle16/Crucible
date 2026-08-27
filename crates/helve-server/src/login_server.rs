//! Product-level R1A Login composition for the Helve development server.
//!
//! Minecraft 26.2 source admission shows the `LoginFinished` session UUID belongs to the server
//! connection population, not to an individual accepted socket. This module therefore owns an
//! explicit [`ServerSessionEpoch`] value which callers create once per listener lifetime and copy
//! into each target-local connection state.
//!
//! The epoch type deliberately does not generate randomness. Entropy/bootstrap policy is a separate
//! product concern and must not leak into `Target26_2` packet semantics. Until that runtime policy is
//! qualified, the development executable accepts an explicit RFC-4122 version-4 epoch.

use std::fmt;
use std::io::{Read, Write};
use std::num::NonZeroUsize;

use crucible_connection_core::ConnectionLimits;
use crucible_preplay_core::PrePlayConnection;
use crucible_preplay_io::{ActionBudget, PrePlayIo, PrePlayIoError, ServiceStop};
use crucible_session_core::SessionPhase;
use crucible_target_26_2::{Target26_2, Target26_2Error, Target26_2State};

const FRAME_BODY_LIMIT: usize = 4 * 1_024;
const INGRESS_LIMIT: usize = 16 * 1_024;
const EGRESS_LIMIT: usize = 16 * 1_024;
const READ_SCRATCH_BYTES: usize = 4 * 1_024;

// One action per service call is deliberate at the current R1A boundary. It lets product
// composition observe the exact Login -> Configuration transition before any already-coalesced
// Configuration bytes are decoded by a target which does not admit that phase yet.
const ACTIONS_PER_SERVICE: usize = 1;

/// One source-faithful server connection-population UUID epoch.
///
/// The value is an RFC-4122 variant, version-4 UUID and is intended to be created once per listener
/// lifetime, then reused for every accepted Login connection attached to that listener.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServerSessionEpoch([u8; 16]);

impl ServerSessionEpoch {
    /// Validates one raw UUID value as RFC-4122 variant/version 4.
    ///
    /// # Errors
    ///
    /// Returns a structural UUID error when the version or variant bits do not match the
    /// source-observed `UUID.randomUUID()` shape.
    pub const fn from_bytes(bytes: [u8; 16]) -> Result<Self, ServerSessionEpochParseError> {
        if bytes[6] >> 4 != 4 {
            return Err(ServerSessionEpochParseError::NotVersion4);
        }
        if bytes[8] >> 6 != 2 {
            return Err(ServerSessionEpochParseError::NotRfc4122Variant);
        }
        Ok(Self(bytes))
    }

    /// Parses exactly 32 hexadecimal UUID digits and validates the source-required UUID shape.
    ///
    /// Hyphens are deliberately not accepted so the development CLI has one canonical spelling.
    ///
    /// # Errors
    ///
    /// Returns a precise parse/UUID-shape error for malformed input.
    pub fn parse_hex(input: &str) -> Result<Self, ServerSessionEpochParseError> {
        let source = input.as_bytes();
        if source.len() != 32 {
            return Err(ServerSessionEpochParseError::Length {
                actual: source.len(),
            });
        }

        let mut bytes = [0_u8; 16];
        let mut output = 0usize;
        while output < bytes.len() {
            let high_index = output * 2;
            let low_index = high_index + 1;
            let high =
                hex_nibble(source[high_index]).ok_or(ServerSessionEpochParseError::InvalidHex {
                    index: high_index,
                    byte: source[high_index],
                })?;
            let low =
                hex_nibble(source[low_index]).ok_or(ServerSessionEpochParseError::InvalidHex {
                    index: low_index,
                    byte: source[low_index],
                })?;
            bytes[output] = (high << 4) | low;
            output += 1;
        }
        Self::from_bytes(bytes)
    }

    /// Returns the exact 16 wire bytes copied into each R1A target state.
    #[must_use]
    pub const fn into_bytes(self) -> [u8; 16] {
        self.0
    }
}

/// Fail-closed development-epoch parser error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServerSessionEpochParseError {
    /// The canonical development spelling must contain exactly 32 hexadecimal digits.
    Length {
        /// Actual byte length supplied by the caller.
        actual: usize,
    },
    /// One byte was not an ASCII hexadecimal digit.
    InvalidHex {
        /// Zero-based input byte index.
        index: usize,
        /// Rejected byte.
        byte: u8,
    },
    /// UUID version bits were not version 4.
    NotVersion4,
    /// UUID variant bits were not RFC-4122 variant `10`.
    NotRfc4122Variant,
}

impl fmt::Display for ServerSessionEpochParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length { actual } => write!(
                formatter,
                "session epoch must contain exactly 32 hexadecimal digits, got {actual}"
            ),
            Self::InvalidHex { index, byte } => write!(
                formatter,
                "session epoch contains non-hex byte 0x{byte:02x} at index {index}"
            ),
            Self::NotVersion4 => formatter.write_str("session epoch UUID must be version 4"),
            Self::NotRfc4122Variant => {
                formatter.write_str("session epoch UUID must use the RFC-4122 variant")
            }
        }
    }
}

/// Why one R1A development connection stopped successfully.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum R1AConnectionExit {
    /// Login acknowledgement committed and the same connection reached Configuration.
    ConfigurationReady {
        /// Already-read bytes intentionally left untouched for the future Configuration target.
        buffered_ingress: usize,
    },
    /// A Status route or another admitted terminal action closed the session.
    SessionClosed,
    /// The peer closed cleanly before reaching Configuration.
    PeerEof,
}

/// Drives one blocking transport through the admitted Status/R1A Login surface.
///
/// The returned Configuration boundary is intentionally strict: one semantic action is committed per
/// service call, so coalesced Configuration bytes remain buffered and unconsumed after
/// `LoginAcknowledged`. Once R1B is admitted, the same owned `PrePlayIo<Target26_2>` architecture can
/// continue through Configuration rather than replacing the transport stack.
///
/// # Errors
///
/// Returns the fail-closed bounded I/O/target error from the existing pre-play stack.
pub fn serve_r1a_blocking_transport<RW>(
    transport: &mut RW,
    session_epoch: ServerSessionEpoch,
) -> Result<R1AConnectionExit, PrePlayIoError<Target26_2Error>>
where
    RW: Read + Write + ?Sized,
{
    let target_state = Target26_2State::with_login_session_uuid(session_epoch.into_bytes());
    let connection = PrePlayConnection::<Target26_2>::with_target_state(limits(), target_state);
    let mut io = PrePlayIo::from_connection(connection, read_scratch_bytes());
    let budget = action_budget();

    loop {
        let report = io.service_once(transport, crate::HELVE_STATUS_JSON, budget)?;

        if io.connection().phase() == SessionPhase::Configuration {
            debug_assert_eq!(io.connection().queued_egress(), 0);
            return Ok(R1AConnectionExit::ConfigurationReady {
                buffered_ingress: io.connection().buffered_ingress(),
            });
        }

        match report.stop {
            ServiceStop::SessionClosed => return Ok(R1AConnectionExit::SessionClosed),
            ServiceStop::PeerEof => return Ok(R1AConnectionExit::PeerEof),
            ServiceStop::InputPending
            | ServiceStop::OutputPending
            | ServiceStop::ActionBudgetExhausted => {}
        }
    }
}

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(FRAME_BODY_LIMIT, INGRESS_LIMIT, EGRESS_LIMIT)
        .expect("R1A product limits are positive and coherent")
}

fn read_scratch_bytes() -> NonZeroUsize {
    NonZeroUsize::new(READ_SCRATCH_BYTES).expect("R1A read scratch is positive")
}

fn action_budget() -> ActionBudget {
    ActionBudget::new(ACTIONS_PER_SERVICE).expect("R1A action budget is positive")
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read, Write};

    use super::{R1AConnectionExit, ServerSessionEpoch, ServerSessionEpochParseError};

    #[allow(
        dead_code,
        unreachable_pub,
        reason = "the composition fixture reuses generated admitted Login evidence directly"
    )]
    mod admitted_login_codegen {
        include!("../../network/crucible-target-26-2/src/generated/login_26_2.rs");
    }

    const GOLDEN_EPOCH_HEX: &str = "4d7f604f196a43b08987f0b2a27c2663";

    #[derive(Debug)]
    struct MemoryTransport {
        input: Vec<u8>,
        cursor: usize,
        output: Vec<u8>,
    }

    impl MemoryTransport {
        fn new(input: Vec<u8>) -> Self {
            Self {
                input,
                cursor: 0,
                output: Vec::new(),
            }
        }
    }

    impl Read for MemoryTransport {
        fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
            if self.cursor == self.input.len() {
                return Ok(0);
            }
            let count = (self.input.len() - self.cursor).min(destination.len());
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

    fn login_stream(extra_after_ack: &[u8]) -> Vec<u8> {
        let mut stream = Vec::new();
        stream.extend_from_slice(
            admitted_login_codegen::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_LOGIN_FRAME,
        );
        stream
            .extend_from_slice(admitted_login_codegen::golden::LOGIN_SERVERBOUND_LOGIN_HELLO_FRAME);
        stream.extend_from_slice(
            admitted_login_codegen::golden::LOGIN_SERVERBOUND_LOGIN_ACKNOWLEDGED_FRAME,
        );
        stream.extend_from_slice(extra_after_ack);
        stream
    }

    #[test]
    fn explicit_epoch_parser_accepts_the_admitted_v4_session_uuid() {
        let epoch = ServerSessionEpoch::parse_hex(GOLDEN_EPOCH_HEX).expect("golden epoch parses");
        assert_eq!(
            epoch.into_bytes(),
            [
                0x4d, 0x7f, 0x60, 0x4f, 0x19, 0x6a, 0x43, 0xb0, 0x89, 0x87, 0xf0, 0xb2, 0xa2, 0x7c,
                0x26, 0x63,
            ]
        );
    }

    #[test]
    fn explicit_epoch_parser_fails_closed_on_non_v4_or_non_rfc_variant() {
        let mut not_v4 = ServerSessionEpoch::parse_hex(GOLDEN_EPOCH_HEX)
            .expect("golden epoch parses")
            .into_bytes();
        not_v4[6] = 0x53;
        assert_eq!(
            ServerSessionEpoch::from_bytes(not_v4),
            Err(ServerSessionEpochParseError::NotVersion4)
        );

        let mut wrong_variant = ServerSessionEpoch::parse_hex(GOLDEN_EPOCH_HEX)
            .expect("golden epoch parses")
            .into_bytes();
        wrong_variant[8] = 0x49;
        assert_eq!(
            ServerSessionEpoch::from_bytes(wrong_variant),
            Err(ServerSessionEpochParseError::NotRfc4122Variant)
        );
    }

    #[test]
    fn product_composition_reaches_configuration_with_byte_exact_login_finished() {
        let epoch = ServerSessionEpoch::parse_hex(GOLDEN_EPOCH_HEX).expect("golden epoch parses");
        let mut transport = MemoryTransport::new(login_stream(&[]));

        assert_eq!(
            super::serve_r1a_blocking_transport(&mut transport, epoch),
            Ok(R1AConnectionExit::ConfigurationReady {
                buffered_ingress: 0,
            })
        );
        assert_eq!(
            transport.output,
            admitted_login_codegen::golden::LOGIN_CLIENTBOUND_LOGIN_FINISHED_FRAME
        );
    }

    #[test]
    fn configuration_bytes_coalesced_after_ack_are_not_consumed_early() {
        let epoch = ServerSessionEpoch::parse_hex(GOLDEN_EPOCH_HEX).expect("golden epoch parses");
        // One syntactically complete framed packet standing in for the not-yet-admitted R1B input.
        let future_configuration_frame = [0x01, 0x00];
        let mut transport = MemoryTransport::new(login_stream(&future_configuration_frame));

        assert_eq!(
            super::serve_r1a_blocking_transport(&mut transport, epoch),
            Ok(R1AConnectionExit::ConfigurationReady {
                buffered_ingress: future_configuration_frame.len(),
            })
        );
    }

    #[test]
    fn one_server_epoch_is_reused_across_independent_connections() {
        let epoch = ServerSessionEpoch::parse_hex(GOLDEN_EPOCH_HEX).expect("golden epoch parses");
        let mut first = MemoryTransport::new(login_stream(&[]));
        let mut second = MemoryTransport::new(login_stream(&[]));

        assert!(matches!(
            super::serve_r1a_blocking_transport(&mut first, epoch),
            Ok(R1AConnectionExit::ConfigurationReady { .. })
        ));
        assert!(matches!(
            super::serve_r1a_blocking_transport(&mut second, epoch),
            Ok(R1AConnectionExit::ConfigurationReady { .. })
        ));
        assert_eq!(first.output, second.output);
        assert_eq!(
            first.output,
            admitted_login_codegen::golden::LOGIN_CLIENTBOUND_LOGIN_FINISHED_FRAME
        );
    }
}
