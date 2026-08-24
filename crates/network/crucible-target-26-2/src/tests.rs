use crucible_connection_core::ConnectionLimits;
use crucible_connection_driver::ConnectionDriver;
use crucible_packet_core::{PacketCodecError, PacketField, PacketWriter};
use crucible_preplay_core::{PrePlayConnection, PrePlayError, PrePlayProcess};
use crucible_session_core::SessionPhase;

use super::{
    LoginState, MAX_R0_PACKET_BODY_BYTES, MAX_SERVER_ADDRESS_UTF16_UNITS, Target26_2,
    Target26_2Error, Target26_2State, generated,
};

const ORACLE_STATUS_JSON: &str = "{\"description\":\"Crucible R0 Oracle\",\"players\":{\"max\":20,\"online\":0},\"version\":{\"name\":\"26.2\",\"protocol\":776},\"enforcesSecureChat\":true}";
const REAL_LOGIN_SESSION_UUID: [u8; 16] = [
    0x4d, 0x7f, 0x60, 0x4f, 0x19, 0x6a, 0x43, 0xb0, 0x89, 0x87, 0xf0, 0xb2, 0xa2, 0x7c, 0x26, 0x63,
];
const REAL_OFFLINE_PROFILE_UUID: [u8; 16] = [
    0x68, 0x20, 0x14, 0xfe, 0xad, 0x63, 0x36, 0x99, 0xaa, 0xda, 0x79, 0xaa, 0x08, 0xd9, 0x5b, 0x45,
];

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(
        MAX_R0_PACKET_BODY_BYTES,
        MAX_R0_PACKET_BODY_BYTES * 2,
        MAX_R0_PACKET_BODY_BYTES * 2,
    )
    .expect("coherent target test limits")
}

fn tight_limits() -> ConnectionLimits {
    ConnectionLimits::new(32, 256, 33).expect("one 32-byte frame fits")
}

fn encoded_frame(body: &[u8], limits: ConnectionLimits) -> Vec<u8> {
    let mut driver = ConnectionDriver::new(limits);
    driver.queue_frame::<()>(body).expect("test frame fits");
    driver.pending_egress().to_vec()
}

fn body(
    packet_id: i32,
    encode: impl FnOnce(&mut PacketWriter) -> Result<(), PacketCodecError>,
) -> Vec<u8> {
    let mut writer = PacketWriter::new(MAX_R0_PACKET_BODY_BYTES).expect("positive packet bound");
    writer.write_var_int(packet_id).expect("packet id fits");
    encode(&mut writer).expect("test payload fits");
    writer.into_bytes()
}

fn handshake_body(protocol: i32, intent: i32) -> Vec<u8> {
    body(
        generated::handshake::serverbound::CLIENT_INTENTION,
        |writer| {
            writer.write_var_int(protocol)?;
            writer.write_string("127.0.0.1", MAX_SERVER_ADDRESS_UTF16_UNITS)?;
            writer.write_u16(25_566)?;
            writer.write_var_int(intent)
        },
    )
}

fn login_state() -> Target26_2State {
    Target26_2State::with_login_session_uuid(REAL_LOGIN_SESSION_UUID)
}

fn enter_status(connection: &mut PrePlayConnection<Target26_2>) {
    connection
        .ingest(generated::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_FRAME)
        .expect("golden handshake ingress");
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Handshake,
            to: SessionPhase::Status,
            outbound_frames: 0,
        })
    );
}

fn enter_login(connection: &mut PrePlayConnection<Target26_2>) {
    connection
        .ingest(generated::login_26_2::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_LOGIN_FRAME)
        .expect("golden Login handshake ingress");
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Handshake,
            to: SessionPhase::Login,
            outbound_frames: 0,
        })
    );
}

fn commit_real_hello(connection: &mut PrePlayConnection<Target26_2>) {
    connection
        .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_HELLO_FRAME)
        .expect("real Login hello ingress");
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Login,
            to: SessionPhase::Login,
            outbound_frames: 1,
        })
    );
}

fn drain(connection: &mut PrePlayConnection<Target26_2>) {
    let queued = connection.queued_egress();
    connection
        .consume_written(queued)
        .expect("drain exact egress");
}

#[test]
fn generated_identity_contains_both_admitted_contracts() {
    assert_eq!(generated::CONTRACT_ID, "PROTO-NET-STATUS-26-2-001");
    assert_eq!(generated::MINECRAFT_VERSION, "26.2");
    assert_eq!(generated::PROTOCOL_VERSION, 776);
    assert_eq!(generated::handshake::serverbound::CLIENT_INTENTION, 0);
    assert_eq!(generated::status::serverbound::STATUS_REQUEST, 0);
    assert_eq!(generated::status::serverbound::PING_REQUEST, 1);
    assert_eq!(generated::status::clientbound::STATUS_RESPONSE, 0);
    assert_eq!(generated::status::clientbound::PONG_RESPONSE, 1);
    assert_eq!(
        generated::login_26_2::CONTRACT_ID,
        "PROTO-NET-LOGIN-26-2-001"
    );
    assert_eq!(generated::login_26_2::PROTOCOL_VERSION, 776);
    assert_eq!(generated::login_26_2::login::serverbound::LOGIN_HELLO, 0);
    assert_eq!(
        generated::login_26_2::login::serverbound::LOGIN_ACKNOWLEDGED,
        3
    );
    assert_eq!(generated::login_26_2::login::clientbound::LOGIN_FINISHED, 2);
}

#[test]
fn real_oracle_status_and_ping_exchange_is_byte_exact() {
    let mut connection = PrePlayConnection::<Target26_2>::new(limits());
    enter_status(&mut connection);
    assert!(!connection.target_state().status_response_sent());

    connection
        .ingest(generated::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME)
        .expect("golden status request ingress");
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Status,
            to: SessionPhase::Status,
            outbound_frames: 1,
        })
    );
    assert!(connection.target_state().status_response_sent());
    assert_eq!(
        connection.pending_egress(),
        generated::golden::STATUS_CLIENTBOUND_STATUS_RESPONSE_FRAME
    );
    drain(&mut connection);

    connection
        .ingest(generated::golden::STATUS_SERVERBOUND_PING_REQUEST_FRAME)
        .expect("golden ping ingress");
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Status,
            to: SessionPhase::Closed,
            outbound_frames: 1,
        })
    );
    assert_eq!(
        connection.pending_egress(),
        generated::golden::STATUS_CLIENTBOUND_PONG_RESPONSE_FRAME
    );
}

#[test]
fn real_client_login_exchange_is_byte_exact_and_retains_profile() {
    let mut connection =
        PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
    enter_login(&mut connection);
    commit_real_hello(&mut connection);

    let LoginState::AwaitAcknowledgement {
        session_uuid,
        profile,
    } = connection.target_state().login
    else {
        panic!("real Login hello must retain accepted profile");
    };
    assert_eq!(session_uuid, REAL_LOGIN_SESSION_UUID);
    assert_eq!(profile.id(), REAL_OFFLINE_PROFILE_UUID);
    assert_eq!(profile.name(), "Stato16");
    assert_eq!(
        connection.pending_egress(),
        generated::login_26_2::golden::LOGIN_CLIENTBOUND_LOGIN_FINISHED_FRAME
    );
    drain(&mut connection);

    connection
        .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_ACKNOWLEDGED_FRAME)
        .expect("real Login acknowledgement ingress");
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Login,
            to: SessionPhase::Configuration,
            outbound_frames: 0,
        })
    );
    let LoginState::AwaitAcknowledgement { profile, .. } = connection.target_state().login else {
        panic!("Configuration entry must retain accepted Login profile");
    };
    assert_eq!(profile.id(), REAL_OFFLINE_PROFILE_UUID);
    assert_eq!(profile.name(), "Stato16");
    assert_eq!(connection.queued_egress(), 0);
}

#[test]
fn status_handshake_remains_protocol_tolerant() {
    let mut connection = PrePlayConnection::<Target26_2>::new(limits());
    let frame = encoded_frame(&handshake_body(775, 1), limits());
    connection.ingest(&frame).expect("handshake ingress");
    assert!(matches!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Handshake,
            to: SessionPhase::Status,
            outbound_frames: 0,
        })
    ));
}

#[test]
fn login_handshake_requires_exact_protocol_transactionally() {
    let mut connection =
        PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
    let frame = encoded_frame(&handshake_body(775, 2), limits());
    connection.ingest(&frame).expect("Login handshake ingress");
    let buffered = connection.buffered_ingress();
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(
            Target26_2Error::LoginProtocolMismatch {
                expected: 776,
                actual: 775,
            }
        ))
    );
    assert_eq!(connection.phase(), SessionPhase::Handshake);
    assert_eq!(connection.buffered_ingress(), buffered);
}

#[test]
fn login_requires_runtime_session_uuid_without_consuming_handshake() {
    let mut connection = PrePlayConnection::<Target26_2>::new(limits());
    connection
        .ingest(generated::login_26_2::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_LOGIN_FRAME)
        .expect("Login handshake ingress");
    let buffered = connection.buffered_ingress();
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(
            Target26_2Error::MissingLoginSessionUuid
        ))
    );
    assert_eq!(connection.phase(), SessionPhase::Handshake);
    assert_eq!(connection.buffered_ingress(), buffered);
}

#[test]
fn unsupported_handshake_intent_is_rejected_without_consuming_input() {
    let mut connection = PrePlayConnection::<Target26_2>::new(limits());
    let frame = encoded_frame(&handshake_body(776, 99), limits());
    connection.ingest(&frame).expect("handshake ingress");
    let buffered = connection.buffered_ingress();
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(Target26_2Error::UnsupportedIntent(99)))
    );
    assert_eq!(connection.phase(), SessionPhase::Handshake);
    assert_eq!(connection.buffered_ingress(), buffered);
}

#[test]
fn invalid_player_name_is_rejected_transactionally() {
    let mut connection =
        PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
    enter_login(&mut connection);
    let malformed = body(
        generated::login_26_2::login::serverbound::LOGIN_HELLO,
        |writer| {
            writer.write_string("bad name", 16)?;
            writer.write_u64(0)?;
            writer.write_u64(0)
        },
    );
    connection
        .ingest(&encoded_frame(&malformed, limits()))
        .expect("invalid-name hello ingress");
    let buffered = connection.buffered_ingress();
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(Target26_2Error::InvalidPlayerName))
    );
    assert_eq!(connection.phase(), SessionPhase::Login);
    assert!(!connection.target_state().login_finished_sent());
    assert_eq!(connection.buffered_ingress(), buffered);
    assert_eq!(connection.queued_egress(), 0);
}

#[test]
fn empty_player_name_matches_vanilla_predicate() {
    let mut connection =
        PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
    enter_login(&mut connection);
    let hello = body(
        generated::login_26_2::login::serverbound::LOGIN_HELLO,
        |writer| {
            writer.write_string("", 16)?;
            writer.write_u64(0)?;
            writer.write_u64(0)
        },
    );
    connection
        .ingest(&encoded_frame(&hello, limits()))
        .expect("empty-name hello ingress");
    assert!(matches!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Login,
            to: SessionPhase::Login,
            outbound_frames: 1,
        })
    ));
}

#[test]
fn login_packet_order_is_fail_closed() {
    let mut early_ack = PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
    enter_login(&mut early_ack);
    early_ack
        .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_ACKNOWLEDGED_FRAME)
        .expect("early acknowledgement ingress");
    let buffered = early_ack.buffered_ingress();
    assert_eq!(
        early_ack.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(Target26_2Error::UnexpectedLoginState))
    );
    assert_eq!(early_ack.buffered_ingress(), buffered);

    let mut duplicate = PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
    enter_login(&mut duplicate);
    commit_real_hello(&mut duplicate);
    drain(&mut duplicate);
    duplicate
        .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_HELLO_FRAME)
        .expect("duplicate hello ingress");
    let buffered = duplicate.buffered_ingress();
    assert_eq!(
        duplicate.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(Target26_2Error::UnexpectedLoginState))
    );
    assert_eq!(duplicate.buffered_ingress(), buffered);
}

#[test]
fn duplicate_status_request_closes_without_second_response() {
    let mut connection = PrePlayConnection::<Target26_2>::new(limits());
    enter_status(&mut connection);
    connection
        .ingest(generated::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME)
        .expect("first status request");
    connection
        .process_one(ORACLE_STATUS_JSON)
        .expect("first status response commits");
    drain(&mut connection);

    connection
        .ingest(generated::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME)
        .expect("duplicate status request");
    assert_eq!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Status,
            to: SessionPhase::Closed,
            outbound_frames: 0,
        })
    );
    assert_eq!(connection.queued_egress(), 0);
    assert_eq!(connection.buffered_ingress(), 0);
}

#[test]
fn nonempty_unit_packets_are_rejected_transactionally() {
    let mut status = PrePlayConnection::<Target26_2>::new(limits());
    enter_status(&mut status);
    let malformed_status = body(generated::status::serverbound::STATUS_REQUEST, |writer| {
        writer.write_bool(true)
    });
    status
        .ingest(&encoded_frame(&malformed_status, limits()))
        .expect("malformed status ingress");
    let buffered = status.buffered_ingress();
    assert_eq!(
        status.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(Target26_2Error::Codec(
            PacketCodecError::TrailingBytes { remaining: 1 }
        )))
    );
    assert_eq!(status.buffered_ingress(), buffered);

    let mut login = PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
    enter_login(&mut login);
    commit_real_hello(&mut login);
    drain(&mut login);
    let malformed_ack = body(
        generated::login_26_2::login::serverbound::LOGIN_ACKNOWLEDGED,
        |writer| writer.write_bool(true),
    );
    login
        .ingest(&encoded_frame(&malformed_ack, limits()))
        .expect("malformed acknowledgement ingress");
    let buffered = login.buffered_ingress();
    assert_eq!(
        login.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(Target26_2Error::Codec(
            PacketCodecError::TrailingBytes { remaining: 1 }
        )))
    );
    assert_eq!(login.phase(), SessionPhase::Login);
    assert_eq!(login.buffered_ingress(), buffered);
}

#[test]
fn truncated_fixed_width_fields_are_rejected_transactionally() {
    let mut login = PrePlayConnection::<Target26_2>::with_target_state(limits(), login_state());
    enter_login(&mut login);
    let truncated_hello = body(
        generated::login_26_2::login::serverbound::LOGIN_HELLO,
        |writer| {
            writer.write_string("Player", 16)?;
            writer.write_u64(0)?;
            writer.write_bytes(&[0_u8; 7])
        },
    );
    login
        .ingest(&encoded_frame(&truncated_hello, limits()))
        .expect("truncated hello ingress");
    let buffered = login.buffered_ingress();
    assert_eq!(
        login.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(Target26_2Error::Codec(
            PacketCodecError::Truncated {
                field: PacketField::U64,
                remaining: 7,
            }
        )))
    );
    assert_eq!(login.buffered_ingress(), buffered);

    let mut status = PrePlayConnection::<Target26_2>::new(limits());
    enter_status(&mut status);
    let truncated_ping = body(generated::status::serverbound::PING_REQUEST, |writer| {
        writer.write_bytes(&[0_u8; 7])
    });
    status
        .ingest(&encoded_frame(&truncated_ping, limits()))
        .expect("truncated ping ingress");
    let buffered = status.buffered_ingress();
    assert_eq!(
        status.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Target(Target26_2Error::Codec(
            PacketCodecError::Truncated {
                field: PacketField::I64,
                remaining: 7,
            }
        )))
    );
    assert_eq!(status.buffered_ingress(), buffered);
}

#[test]
fn signed_ping_payload_is_echoed_bit_exact_then_closes() {
    let mut connection = PrePlayConnection::<Target26_2>::new(limits());
    enter_status(&mut connection);
    let ping = body(generated::status::serverbound::PING_REQUEST, |writer| {
        writer.write_i64(-0x0102_0304_0506_0708_i64)
    });
    let expected = body(generated::status::clientbound::PONG_RESPONSE, |writer| {
        writer.write_i64(-0x0102_0304_0506_0708_i64)
    });
    connection
        .ingest(&encoded_frame(&ping, limits()))
        .expect("signed ping ingress");
    assert!(matches!(
        connection.process_one(ORACLE_STATUS_JSON),
        Ok(PrePlayProcess::Committed {
            from: SessionPhase::Status,
            to: SessionPhase::Closed,
            outbound_frames: 1,
        })
    ));
    assert_eq!(
        connection.pending_egress(),
        encoded_frame(&expected, limits())
    );
}

#[test]
fn egress_rejection_rolls_back_status_state_and_input() {
    let limits = tight_limits();
    let mut connection = PrePlayConnection::<Target26_2>::new(limits);
    connection
        .ingest(generated::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_FRAME)
        .expect("small-limit handshake ingress");
    connection
        .process_one(ORACLE_STATUS_JSON)
        .expect("small-limit handshake commits");
    connection
        .ingest(generated::golden::STATUS_SERVERBOUND_STATUS_REQUEST_FRAME)
        .expect("status request ingress");
    let buffered = connection.buffered_ingress();
    assert!(matches!(
        connection.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Buffer(_))
    ));
    assert!(!connection.target_state().status_response_sent());
    assert_eq!(connection.buffered_ingress(), buffered);
    assert_eq!(connection.queued_egress(), 0);
}

#[test]
fn egress_rejection_rolls_back_login_stage_profile_and_input() {
    let limits = tight_limits();
    let mut connection = PrePlayConnection::<Target26_2>::with_target_state(limits, login_state());
    connection
        .ingest(generated::login_26_2::golden::HANDSHAKE_SERVERBOUND_CLIENT_INTENTION_LOGIN_FRAME)
        .expect("small-limit Login handshake ingress");
    connection
        .process_one(ORACLE_STATUS_JSON)
        .expect("small-limit Login handshake commits");
    connection
        .ingest(generated::login_26_2::golden::LOGIN_SERVERBOUND_LOGIN_HELLO_FRAME)
        .expect("Login hello ingress");
    let buffered = connection.buffered_ingress();
    assert!(matches!(
        connection.process_one(ORACLE_STATUS_JSON),
        Err(PrePlayError::Buffer(_))
    ));
    assert_eq!(connection.phase(), SessionPhase::Login);
    assert!(!connection.target_state().login_finished_sent());
    assert!(matches!(
        connection.target_state().login,
        LoginState::AwaitHello { .. }
    ));
    assert_eq!(connection.buffered_ingress(), buffered);
    assert_eq!(connection.queued_egress(), 0);
}
