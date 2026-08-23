//! Source-backed Minecraft Java 26.2 / protocol 776 handshake and status law.

#![forbid(unsafe_code)]

use crucible_protocol_core::{
    DecodeResult, MAX_FRAME_BODY_LEN, WireError, decode_string, decode_var_int, encode_frame,
    encode_string, encode_var_int,
};

/// Minecraft Java version implemented by this packet-law crate.
pub const MINECRAFT_VERSION: &str = "26.2";
/// Network protocol version implemented by this packet-law crate.
pub const PROTOCOL_VERSION: i32 = 776;
/// Maximum host length from `ClientIntentionPacket`.
pub const MAX_HANDSHAKE_HOST_UTF16: usize = 255;
/// Maximum status JSON length from `ClientboundStatusResponsePacket`.
pub const MAX_STATUS_JSON_UTF16: usize = 32_767;

/// HANDSHAKING serverbound `intention` registration index.
pub const HANDSHAKE_INTENTION_ID: i32 = 0;
/// STATUS serverbound `status_request` registration index.
pub const STATUS_REQUEST_ID: i32 = 0;
/// STATUS serverbound `ping_request` registration index.
pub const STATUS_PING_ID: i32 = 1;
/// STATUS clientbound `status_response` registration index.
pub const STATUS_RESPONSE_ID: i32 = 0;
/// STATUS clientbound `pong_response` registration index.
pub const STATUS_PONG_ID: i32 = 1;

/// Source-backed connection intention values from Minecraft 26.2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientIntent {
    /// Query server-list status.
    Status,
    /// Begin normal login.
    Login,
    /// Begin a transfer login.
    Transfer,
}

impl ClientIntent {
    /// Returns the exact protocol integer used on the wire.
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Status => 1,
            Self::Login => 2,
            Self::Transfer => 3,
        }
    }

    fn from_id(id: i32) -> Result<Self, ProtocolError> {
        match id {
            1 => Ok(Self::Status),
            2 => Ok(Self::Login),
            3 => Ok(Self::Transfer),
            _ => Err(ProtocolError::UnknownIntent(id)),
        }
    }
}

/// Borrowed view of the sole HANDSHAKING serverbound packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientIntention<'a> {
    /// Protocol version advertised by the peer.
    pub protocol_version: i32,
    /// Host string supplied by the peer.
    pub host_name: &'a str,
    /// Unsigned TCP port from the handshake payload.
    pub port: u16,
    /// Requested next protocol.
    pub intention: ClientIntent,
}

/// Source-backed STATUS serverbound packet set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusServerbound {
    /// Empty status request, network ID 0.
    Request,
    /// Ping request, network ID 1, preserving all 64 payload bits.
    Ping(i64),
}

/// Action emitted by the pure STATUS connection state machine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusAction {
    /// Send one current status response and keep the connection open for ping.
    SendStatus,
    /// Echo the ping value, then close the status connection.
    SendPongAndClose(i64),
    /// Close without another status response, e.g. after a duplicate request.
    Close,
}

/// Fail-closed packet-law errors for the 26.2 handshake/status slice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    /// Lower-level wire law failed.
    Wire(WireError),
    /// A complete frame did not contain enough bytes for its declared packet shape.
    TruncatedPacket { packet: &'static str },
    /// Packet ID was not registered in the active source-backed protocol.
    UnknownPacketId { protocol: &'static str, id: i32 },
    /// A complete packet retained unread bytes after its declared fields.
    TrailingBytes { packet: &'static str, count: usize },
    /// Packet payload had a fixed width different from the source-backed width.
    InvalidPayloadLength {
        packet: &'static str,
        expected: usize,
        actual: usize,
    },
    /// Handshake intention was not one of 1, 2 or 3.
    UnknownIntent(i32),
    /// The status-only integration path received a non-status intention.
    NonStatusIntent(ClientIntent),
    /// Input was supplied after the pure status session had closed.
    StatusSessionClosed,
}

impl From<WireError> for ProtocolError {
    fn from(value: WireError) -> Self {
        Self::Wire(value)
    }
}

/// Decodes the complete body of the HANDSHAKING `intention` packet.
///
/// The body starts at packet ID; the outer `VarInt21` frame has already been removed. This function
/// deliberately does not require `protocol_version == 776`: vanilla's STATUS branch does not apply
/// the login version gate.
///
/// # Errors
///
/// Returns an error for any wire violation, nonzero packet ID, unknown intention, truncation or
/// trailing bytes.
pub fn decode_client_intention(body: &[u8]) -> Result<ClientIntention<'_>, ProtocolError> {
    let mut cursor = 0_usize;
    let packet_id = take_var_int(body, &mut cursor, "handshake intention")?;
    if packet_id != HANDSHAKE_INTENTION_ID {
        return Err(ProtocolError::UnknownPacketId {
            protocol: "handshake",
            id: packet_id,
        });
    }

    let protocol_version = take_var_int(body, &mut cursor, "handshake intention")?;
    let host_name = take_string(
        body,
        &mut cursor,
        MAX_HANDSHAKE_HOST_UTF16,
        "handshake intention",
    )?;
    let port_end = cursor
        .checked_add(2)
        .ok_or(ProtocolError::TruncatedPacket {
            packet: "handshake intention",
        })?;
    let port_bytes = body
        .get(cursor..port_end)
        .ok_or(ProtocolError::TruncatedPacket {
            packet: "handshake intention",
        })?;
    let port = u16::from_be_bytes([port_bytes[0], port_bytes[1]]);
    cursor = port_end;
    let intention = ClientIntent::from_id(take_var_int(
        body,
        &mut cursor,
        "handshake intention",
    )?)?;
    ensure_consumed(body, cursor, "handshake intention")?;

    Ok(ClientIntention {
        protocol_version,
        host_name,
        port,
        intention,
    })
}

/// Decodes one complete STATUS serverbound packet body.
///
/// # Errors
///
/// Returns an error for an unknown packet ID, malformed packet payload, or unread suffix.
pub fn decode_status_serverbound(body: &[u8]) -> Result<StatusServerbound, ProtocolError> {
    let mut cursor = 0_usize;
    let packet_id = take_var_int(body, &mut cursor, "status packet")?;
    match packet_id {
        STATUS_REQUEST_ID => {
            ensure_consumed(body, cursor, "status request")?;
            Ok(StatusServerbound::Request)
        }
        STATUS_PING_ID => {
            let payload = body.get(cursor..).ok_or(ProtocolError::TruncatedPacket {
                packet: "ping request",
            })?;
            if payload.len() != 8 {
                return Err(ProtocolError::InvalidPayloadLength {
                    packet: "ping request",
                    expected: 8,
                    actual: payload.len(),
                });
            }
            let bytes: [u8; 8] =
                payload
                    .try_into()
                    .map_err(|_| ProtocolError::InvalidPayloadLength {
                        packet: "ping request",
                        expected: 8,
                        actual: payload.len(),
                    })?;
            Ok(StatusServerbound::Ping(i64::from_be_bytes(bytes)))
        }
        id => Err(ProtocolError::UnknownPacketId {
            protocol: "status",
            id,
        }),
    }
}

/// Encodes one STATUS clientbound status-response packet including its outer TCP frame.
///
/// The temporary body allocation is intentionally confined to this cold server-list path. The
/// eventual gameplay encoder can reserve frame headroom and avoid this extra buffer after separate
/// performance qualification.
///
/// # Errors
///
/// Returns an error if the JSON string violates Minecraft string or frame bounds.
pub fn encode_status_response_frame(
    status_json: &str,
    output: &mut Vec<u8>,
) -> Result<(), ProtocolError> {
    let mut body = Vec::with_capacity(status_json.len().saturating_add(6));
    encode_var_int(STATUS_RESPONSE_ID, &mut body);
    encode_string(status_json, MAX_STATUS_JSON_UTF16, &mut body)?;
    encode_frame(&body, MAX_FRAME_BODY_LEN, output)?;
    Ok(())
}

/// Encodes one STATUS clientbound pong packet including its outer TCP frame.
///
/// # Errors
///
/// Returns an error only if the fixed nine-byte body is rejected by the generic frame law.
pub fn encode_pong_frame(time: i64, output: &mut Vec<u8>) -> Result<(), ProtocolError> {
    let mut body = [0_u8; 9];
    body[0] = u8::try_from(STATUS_PONG_ID).map_err(|_| ProtocolError::UnknownPacketId {
        protocol: "status-clientbound",
        id: STATUS_PONG_ID,
    })?;
    body[1..].copy_from_slice(&time.to_be_bytes());
    encode_frame(&body, MAX_FRAME_BODY_LEN, output)?;
    Ok(())
}

/// Pure state machine for the 26.2 server-side STATUS listener behavior.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatusSession {
    requested_status: bool,
    closed: bool,
}

impl StatusSession {
    /// Creates a fresh status listener immediately after a STATUS handshake.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            requested_status: false,
            closed: false,
        }
    }

    /// Whether this session has entered a terminal disconnected state.
    #[must_use]
    pub const fn is_closed(self) -> bool {
        self.closed
    }

    /// Applies one source-backed STATUS serverbound packet.
    ///
    /// # Errors
    ///
    /// Returns an error if a packet is supplied after the terminal close action.
    pub fn handle(&mut self, packet: StatusServerbound) -> Result<StatusAction, ProtocolError> {
        if self.closed {
            return Err(ProtocolError::StatusSessionClosed);
        }
        match packet {
            StatusServerbound::Request if self.requested_status => {
                self.closed = true;
                Ok(StatusAction::Close)
            }
            StatusServerbound::Request => {
                self.requested_status = true;
                Ok(StatusAction::SendStatus)
            }
            StatusServerbound::Ping(time) => {
                self.closed = true;
                Ok(StatusAction::SendPongAndClose(time))
            }
        }
    }
}

fn take_var_int(
    input: &[u8],
    cursor: &mut usize,
    packet: &'static str,
) -> Result<i32, ProtocolError> {
    let tail = input
        .get(*cursor..)
        .ok_or(ProtocolError::TruncatedPacket { packet })?;
    let DecodeResult::Complete { value, consumed } = decode_var_int(tail)? else {
        return Err(ProtocolError::TruncatedPacket { packet });
    };
    *cursor = (*cursor)
        .checked_add(consumed)
        .ok_or(ProtocolError::TruncatedPacket { packet })?;
    Ok(value)
}

fn take_string<'a>(
    input: &'a [u8],
    cursor: &mut usize,
    max_utf16_units: usize,
    packet: &'static str,
) -> Result<&'a str, ProtocolError> {
    let tail = input
        .get(*cursor..)
        .ok_or(ProtocolError::TruncatedPacket { packet })?;
    let DecodeResult::Complete { value, consumed } = decode_string(tail, max_utf16_units)? else {
        return Err(ProtocolError::TruncatedPacket { packet });
    };
    *cursor = (*cursor)
        .checked_add(consumed)
        .ok_or(ProtocolError::TruncatedPacket { packet })?;
    Ok(value)
}

fn ensure_consumed(
    input: &[u8],
    cursor: usize,
    packet: &'static str,
) -> Result<(), ProtocolError> {
    let remaining = input.len().saturating_sub(cursor);
    if remaining == 0 {
        Ok(())
    } else {
        Err(ProtocolError::TrailingBytes {
            packet,
            count: remaining,
        })
    }
}

#[cfg(test)]
mod tests {
    use crucible_protocol_core::{DecodeResult, decode_frame, decode_string, decode_var_int};

    use super::{
        ClientIntent, MAX_STATUS_JSON_UTF16, MINECRAFT_VERSION, PROTOCOL_VERSION, ProtocolError,
        STATUS_PONG_ID, StatusAction, StatusServerbound, StatusSession, decode_client_intention,
        decode_status_serverbound, encode_pong_frame, encode_status_response_frame,
    };

    const FIXTURES: &str =
        include_str!("../../../../vanilla/fixtures/protocol/26.2-status-fixtures.txt");

    #[test]
    fn source_fixture_pin_matches_crate_constants() {
        assert!(FIXTURES.contains("CRUCIBLE-PROTOCOL-STATUS-FIXTURE|1|26.2|776|4903"));
        assert!(FIXTURES.contains(
            "PROVENANCE|1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750"
        ));
        assert_eq!(MINECRAFT_VERSION, "26.2");
        assert_eq!(PROTOCOL_VERSION, 776);
    }

    #[test]
    fn golden_status_handshake_decodes_exactly() {
        let frame = hex_bytes("10008806096c6f63616c686f737463dd01");
        let DecodeResult::Complete {
            value: body,
            consumed,
        } = decode_frame(&frame, 1_024).expect("valid source fixture")
        else {
            panic!("fixture must be complete");
        };
        assert_eq!(consumed, frame.len());
        let handshake = decode_client_intention(body).expect("valid handshake");
        assert_eq!(handshake.protocol_version, 776);
        assert_eq!(handshake.host_name, "localhost");
        assert_eq!(handshake.port, 25_565);
        assert_eq!(handshake.intention, ClientIntent::Status);
    }

    #[test]
    fn status_handshake_does_not_apply_login_version_gate() {
        let body = hex_bytes("002f096c6f63616c686f737463dd01");
        let handshake =
            decode_client_intention(&body).expect("status handshake is structurally valid");
        assert_eq!(handshake.protocol_version, 47);
        assert_eq!(handshake.intention, ClientIntent::Status);
    }

    #[test]
    fn handshake_rejects_unknown_intent_and_trailing_bytes() {
        let unknown = hex_bytes("008806096c6f63616c686f737463dd04");
        assert_eq!(
            decode_client_intention(&unknown),
            Err(ProtocolError::UnknownIntent(4))
        );

        let trailing = hex_bytes("008806096c6f63616c686f737463dd0100");
        assert_eq!(
            decode_client_intention(&trailing),
            Err(ProtocolError::TrailingBytes {
                packet: "handshake intention",
                count: 1
            })
        );
    }

    #[test]
    fn golden_status_request_and_ping_decode() {
        assert_eq!(
            decode_status_serverbound(&[0]),
            Ok(StatusServerbound::Request)
        );
        let ping_body = hex_bytes("010102030405060708");
        assert_eq!(
            decode_status_serverbound(&ping_body),
            Ok(StatusServerbound::Ping(0x0102_0304_0506_0708))
        );
    }

    #[test]
    fn pong_matches_source_fixture_byte_for_byte() {
        let mut encoded = Vec::new();
        encode_pong_frame(0x0102_0304_0506_0708, &mut encoded).expect("valid pong");
        assert_eq!(encoded, hex_bytes("09010102030405060708"));
        assert_eq!(STATUS_PONG_ID, 1);
    }

    #[test]
    fn status_response_roundtrips_packet_id_and_json() {
        let json = r#"{"description":{"text":"Crucible"},"version":{"name":"26.2","protocol":776}}"#;
        let mut encoded = Vec::new();
        encode_status_response_frame(json, &mut encoded).expect("valid status response");
        let DecodeResult::Complete {
            value: body,
            consumed,
        } = decode_frame(&encoded, 1_024).expect("valid frame")
        else {
            panic!("frame must be complete");
        };
        assert_eq!(consumed, encoded.len());
        let DecodeResult::Complete {
            value: packet_id,
            consumed: id_bytes,
        } = decode_var_int(body).expect("packet id")
        else {
            panic!("packet id must be complete");
        };
        assert_eq!(packet_id, 0);
        let DecodeResult::Complete {
            value: decoded_json,
            consumed: json_bytes,
        } = decode_string(&body[id_bytes..], MAX_STATUS_JSON_UTF16).expect("status json")
        else {
            panic!("status json must be complete");
        };
        assert_eq!(decoded_json, json);
        assert_eq!(id_bytes + json_bytes, body.len());
    }

    #[test]
    fn status_session_matches_vanilla_duplicate_and_ping_behavior() {
        let mut session = StatusSession::new();
        assert_eq!(
            session.handle(StatusServerbound::Request),
            Ok(StatusAction::SendStatus)
        );
        assert_eq!(
            session.handle(StatusServerbound::Request),
            Ok(StatusAction::Close)
        );
        assert!(session.is_closed());
        assert_eq!(
            session.handle(StatusServerbound::Ping(7)),
            Err(ProtocolError::StatusSessionClosed)
        );

        let mut ping_first = StatusSession::new();
        assert_eq!(
            ping_first.handle(StatusServerbound::Ping(-1)),
            Ok(StatusAction::SendPongAndClose(-1))
        );
        assert!(ping_first.is_closed());
    }

    #[test]
    fn status_packet_shapes_fail_closed() {
        assert_eq!(
            decode_status_serverbound(&[0, 1]),
            Err(ProtocolError::TrailingBytes {
                packet: "status request",
                count: 1
            })
        );
        assert_eq!(
            decode_status_serverbound(&[1, 0]),
            Err(ProtocolError::InvalidPayloadLength {
                packet: "ping request",
                expected: 8,
                actual: 1
            })
        );
        assert_eq!(
            decode_status_serverbound(&[2]),
            Err(ProtocolError::UnknownPacketId {
                protocol: "status",
                id: 2
            })
        );
    }

    fn hex_bytes(input: &str) -> Vec<u8> {
        assert_eq!(input.len() % 2, 0, "hex fixture must contain byte pairs");
        input
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let text = core::str::from_utf8(pair).expect("fixture is ASCII");
                u8::from_str_radix(text, 16).expect("fixture contains valid hex")
            })
            .collect()
    }
}
