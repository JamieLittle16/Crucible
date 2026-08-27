//! Qualification-only composition tests for Crucible's generic pre-play client spine.
//!
//! The permanent suite deliberately uses synthetic packet identities and payloads. It proves the
//! transaction boundary between framed transport, packet-body decoding and session transitions
//! before any Minecraft target-version packet identity is admitted.

#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use helve_connection_core::{ConnectionLimits, FrameView};
    use helve_connection_driver::{
        ConnectionDriver, DriverError, FrameBudget, FrameFlow, StopReason,
    };
    use helve_packet_core::{PacketCodecError, PacketReader, PacketWriter};
    use helve_session_core::{SessionPhase, SessionState, SessionStateError};

    const SELECT_STATUS: i32 = 0x51;
    const SELECT_LOGIN: i32 = 0x52;
    const LOGIN_COMPLETE: i32 = 0x53;
    const CONFIG_COMPLETE: i32 = 0x54;
    const CLOSE: i32 = 0x55;
    const STATUS_QUERY: i32 = 0x56;
    const STATUS_REPLY: i32 = 0x71;

    const STATUS_MAGIC: &str = "helve-synthetic-status";
    const LOGIN_MAGIC: &str = "helve-synthetic-login";
    const LOGIN_PROOF: i64 = 0x1122_3344_5566_7788;
    const MAX_SYNTHETIC_STRING_UNITS: usize = 64;
    const MAX_PACKET_BODY: usize = 1_024;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum SyntheticError {
        WrongPhase { packet_id: i32, phase: SessionPhase },
        UnknownPacket(i32),
        Codec(PacketCodecError),
        Transition(SessionStateError),
        InvalidMagic,
        InvalidLoginProof,
        InvalidConfigurationMarker,
        ClosedWithoutTransition,
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
        state: SessionState,
        response: Option<Vec<u8>>,
    }

    fn limits() -> ConnectionLimits {
        ConnectionLimits::new(MAX_PACKET_BODY, 64 * 1_024, 64 * 1_024)
            .expect("coherent qualification limits")
    }

    fn one_frame() -> FrameBudget {
        FrameBudget::new(1).expect("positive frame budget")
    }

    fn packet_body(
        packet_id: i32,
        encode_payload: impl FnOnce(&mut PacketWriter) -> Result<(), PacketCodecError>,
    ) -> Vec<u8> {
        let mut writer = PacketWriter::new(MAX_PACKET_BODY).expect("positive packet bound");
        writer.write_var_int(packet_id).expect("synthetic id fits");
        encode_payload(&mut writer).expect("synthetic payload fits");
        writer.into_bytes()
    }

    fn encoded_frame(body: &[u8]) -> Vec<u8> {
        let mut driver = ConnectionDriver::new(limits());
        driver
            .queue_frame::<()>(body)
            .expect("synthetic frame fits qualification limits");
        driver.pending_egress().to_vec()
    }

    fn select_status_body() -> Vec<u8> {
        packet_body(SELECT_STATUS, |writer| {
            writer.write_string(STATUS_MAGIC, MAX_SYNTHETIC_STRING_UNITS)
        })
    }

    fn status_query_body(value: u64) -> Vec<u8> {
        packet_body(STATUS_QUERY, |writer| writer.write_u64(value))
    }

    fn select_login_body() -> Vec<u8> {
        packet_body(SELECT_LOGIN, |writer| {
            writer.write_string(LOGIN_MAGIC, MAX_SYNTHETIC_STRING_UNITS)
        })
    }

    fn login_complete_body() -> Vec<u8> {
        packet_body(LOGIN_COMPLETE, |writer| writer.write_i64(LOGIN_PROOF))
    }

    fn config_complete_body() -> Vec<u8> {
        packet_body(CONFIG_COMPLETE, |writer| writer.write_bool(true))
    }

    fn close_body() -> Vec<u8> {
        packet_body(CLOSE, |_| Ok(()))
    }

    fn require_phase(
        phase: SessionPhase,
        expected: SessionPhase,
        packet_id: i32,
    ) -> Result<(), SyntheticError> {
        if phase == expected {
            Ok(())
        } else {
            Err(SyntheticError::WrongPhase { packet_id, phase })
        }
    }

    fn decode_action(state: SessionState, frame: FrameView<'_>) -> Result<Action, SyntheticError> {
        let packet_id = frame.packet_id();
        let mut candidate = state;
        let mut reader = PacketReader::new(frame.payload());
        let response = match packet_id {
            SELECT_STATUS => {
                require_phase(state.phase(), SessionPhase::Handshake, packet_id)?;
                if reader.read_string(MAX_SYNTHETIC_STRING_UNITS)? != STATUS_MAGIC {
                    return Err(SyntheticError::InvalidMagic);
                }
                reader.finish()?;
                candidate.advance(SessionPhase::Status)?;
                None
            }
            STATUS_QUERY => {
                require_phase(state.phase(), SessionPhase::Status, packet_id)?;
                let value = reader.read_u64()?;
                reader.finish()?;
                Some(packet_body(STATUS_REPLY, |writer| writer.write_u64(value)))
            }
            SELECT_LOGIN => {
                require_phase(state.phase(), SessionPhase::Handshake, packet_id)?;
                if reader.read_string(MAX_SYNTHETIC_STRING_UNITS)? != LOGIN_MAGIC {
                    return Err(SyntheticError::InvalidMagic);
                }
                reader.finish()?;
                candidate.advance(SessionPhase::Login)?;
                None
            }
            LOGIN_COMPLETE => {
                require_phase(state.phase(), SessionPhase::Login, packet_id)?;
                if reader.read_i64()? != LOGIN_PROOF {
                    return Err(SyntheticError::InvalidLoginProof);
                }
                reader.finish()?;
                candidate.advance(SessionPhase::Configuration)?;
                None
            }
            CONFIG_COMPLETE => {
                require_phase(state.phase(), SessionPhase::Configuration, packet_id)?;
                if !reader.read_bool()? {
                    return Err(SyntheticError::InvalidConfigurationMarker);
                }
                reader.finish()?;
                candidate.advance(SessionPhase::Play)?;
                None
            }
            CLOSE => {
                if state.phase() == SessionPhase::Closed {
                    return Err(SyntheticError::WrongPhase {
                        packet_id,
                        phase: state.phase(),
                    });
                }
                reader.finish()?;
                if !candidate.close() {
                    return Err(SyntheticError::ClosedWithoutTransition);
                }
                None
            }
            value => return Err(SyntheticError::UnknownPacket(value)),
        };
        Ok(Action {
            state: candidate,
            response,
        })
    }

    fn process_one(
        driver: &mut ConnectionDriver,
        state: &mut SessionState,
    ) -> Result<StopReason, DriverError<SyntheticError>> {
        let before = *state;
        let mut action = None;
        let report = driver.process_available(one_frame(), |frame| {
            action = Some(decode_action(before, frame)?);
            Ok(FrameFlow::Yield)
        })?;
        if let Some(action) = action {
            if let Some(response) = action.response {
                driver.queue_frame::<SyntheticError>(&response)?;
            }
            *state = action.state;
        }
        Ok(report.stop)
    }

    fn drain(driver: &mut ConnectionDriver, state: &mut SessionState) -> Result<(), String> {
        loop {
            match process_one(driver, state) {
                Ok(StopReason::Incomplete) => return Ok(()),
                Ok(StopReason::HandlerYield | StopReason::BudgetExhausted) => {}
                Err(error) => {
                    return Err(format!("synthetic spine rejected valid stream: {error:?}"));
                }
            }
        }
    }

    fn feed_fragmented(
        encoded: &[u8],
        fragment: usize,
        state: &mut SessionState,
    ) -> Result<ConnectionDriver, String> {
        let mut driver = ConnectionDriver::new(limits());
        for chunk in encoded.chunks(fragment) {
            driver
                .ingest::<SyntheticError>(chunk)
                .map_err(|error| format!("ingress rejected valid fragment: {error:?}"))?;
            drain(&mut driver, state)?;
        }
        drain(&mut driver, state)?;
        Ok(driver)
    }

    fn login_route_stream() -> Vec<u8> {
        let bodies = [
            select_login_body(),
            login_complete_body(),
            config_complete_body(),
            close_body(),
        ];
        let mut stream = Vec::new();
        for body in &bodies {
            stream.extend_from_slice(&encoded_frame(body));
        }
        stream
    }

    #[test]
    fn whole_frame_and_byte_fragmentation_produce_identical_login_history() {
        let stream = login_route_stream();
        let mut whole_state = SessionState::new();
        let whole = feed_fragmented(&stream, stream.len(), &mut whole_state).expect("whole stream");
        let mut byte_state = SessionState::new();
        let bytes = feed_fragmented(&stream, 1, &mut byte_state).expect("byte stream");

        assert_eq!(whole_state, byte_state);
        assert_eq!(whole_state.phase(), SessionPhase::Closed);
        assert_eq!(whole.buffered_ingress(), 0);
        assert_eq!(bytes.buffered_ingress(), 0);
        assert_eq!(whole.queued_egress(), 0);
        assert_eq!(bytes.queued_egress(), 0);
    }

    #[test]
    fn every_split_point_preserves_state_until_a_complete_frame_exists() {
        for body in [select_status_body(), select_login_body()] {
            let encoded = encoded_frame(&body);
            for split in 0..encoded.len() {
                let mut driver = ConnectionDriver::new(limits());
                let mut state = SessionState::new();
                driver
                    .ingest::<SyntheticError>(&encoded[..split])
                    .expect("prefix fits");
                let buffered_before = driver.buffered_ingress();
                assert_eq!(
                    process_one(&mut driver, &mut state),
                    Ok(StopReason::Incomplete)
                );
                assert_eq!(state.phase(), SessionPhase::Handshake);
                assert_eq!(driver.buffered_ingress(), buffered_before);

                driver
                    .ingest::<SyntheticError>(&encoded[split..])
                    .expect("suffix fits");
                assert_eq!(
                    process_one(&mut driver, &mut state),
                    Ok(StopReason::HandlerYield)
                );
                assert_eq!(driver.buffered_ingress(), 0);
            }
        }
    }

    #[test]
    fn coalesced_status_sequence_preserves_order_and_exact_response_bytes() {
        let nonce = 0xDEAD_BEEF_CAFE_BABEu64;
        let mut stream = encoded_frame(&select_status_body());
        stream.extend_from_slice(&encoded_frame(&status_query_body(nonce)));
        stream.extend_from_slice(&encoded_frame(&close_body()));

        let mut state = SessionState::new();
        let mut driver =
            feed_fragmented(&stream, stream.len(), &mut state).expect("coalesced status");
        assert_eq!(state.phase(), SessionPhase::Closed);
        assert_eq!(driver.buffered_ingress(), 0);
        assert!(driver.queued_egress() > 0);

        let response_bytes = driver.pending_egress().to_vec();
        driver
            .consume_written::<SyntheticError>(response_bytes.len())
            .expect("ack response bytes");
        assert_eq!(driver.queued_egress(), 0);

        let mut decoder = ConnectionDriver::new(limits());
        decoder
            .ingest::<SyntheticError>(&response_bytes)
            .expect("response frame fits");
        let mut observed = None;
        decoder
            .process_available(one_frame(), |frame| {
                assert_eq!(frame.packet_id(), STATUS_REPLY);
                let mut reader = PacketReader::new(frame.payload());
                observed = Some(reader.read_u64().expect("reply nonce"));
                reader.finish().expect("exact reply payload");
                Ok::<_, SyntheticError>(FrameFlow::Yield)
            })
            .expect("response decodes");
        assert_eq!(observed, Some(nonce));
        assert_eq!(decoder.buffered_ingress(), 0);
    }

    #[test]
    fn malformed_complete_payload_does_not_advance_or_consume() {
        let malformed = packet_body(SELECT_STATUS, |writer| {
            writer.write_string(STATUS_MAGIC, MAX_SYNTHETIC_STRING_UNITS)?;
            writer.write_bool(true)
        });
        let encoded = encoded_frame(&malformed);
        let mut driver = ConnectionDriver::new(limits());
        let mut state = SessionState::new();
        driver
            .ingest::<SyntheticError>(&encoded)
            .expect("malformed frame still fits bounds");
        let ingress_before = driver.buffered_ingress();
        let egress_before = driver.queued_egress();

        assert!(matches!(
            process_one(&mut driver, &mut state),
            Err(DriverError::Handler(SyntheticError::Codec(
                PacketCodecError::TrailingBytes { .. }
            )))
        ));
        assert_eq!(state.phase(), SessionPhase::Handshake);
        assert_eq!(driver.buffered_ingress(), ingress_before);
        assert_eq!(driver.queued_egress(), egress_before);
    }

    #[test]
    fn truncated_complete_field_does_not_advance_or_consume() {
        let body = packet_body(SELECT_STATUS, |writer| writer.write_bytes(&[5, b'a', b'b']));
        let encoded = encoded_frame(&body);
        let mut driver = ConnectionDriver::new(limits());
        let mut state = SessionState::new();
        driver
            .ingest::<SyntheticError>(&encoded)
            .expect("truncated semantic packet fits wire bounds");
        let ingress_before = driver.buffered_ingress();

        assert!(matches!(
            process_one(&mut driver, &mut state),
            Err(DriverError::Handler(SyntheticError::Codec(
                PacketCodecError::Truncated { .. }
            )))
        ));
        assert_eq!(state.phase(), SessionPhase::Handshake);
        assert_eq!(driver.buffered_ingress(), ingress_before);
        assert_eq!(driver.queued_egress(), 0);
    }

    #[test]
    fn illegal_packet_for_phase_does_not_consume_or_queue_response() {
        let encoded = encoded_frame(&config_complete_body());
        let mut driver = ConnectionDriver::new(limits());
        let mut state = SessionState::new();
        driver
            .ingest::<SyntheticError>(&encoded)
            .expect("illegal semantic packet fits wire bounds");
        let ingress_before = driver.buffered_ingress();

        assert_eq!(
            process_one(&mut driver, &mut state),
            Err(DriverError::Handler(SyntheticError::WrongPhase {
                packet_id: CONFIG_COMPLETE,
                phase: SessionPhase::Handshake,
            }))
        );
        assert_eq!(state.phase(), SessionPhase::Handshake);
        assert_eq!(driver.buffered_ingress(), ingress_before);
        assert_eq!(driver.queued_egress(), 0);
    }

    #[test]
    fn successful_action_consumes_exactly_the_current_frame() {
        let first = encoded_frame(&select_login_body());
        let second = encoded_frame(&login_complete_body());
        let mut stream = first.clone();
        stream.extend_from_slice(&second);
        let mut driver = ConnectionDriver::new(limits());
        let mut state = SessionState::new();
        driver
            .ingest::<SyntheticError>(&stream)
            .expect("coalesced route fits");

        assert_eq!(
            process_one(&mut driver, &mut state),
            Ok(StopReason::HandlerYield)
        );
        assert_eq!(state.phase(), SessionPhase::Login);
        assert_eq!(driver.buffered_ingress(), second.len());

        assert_eq!(
            process_one(&mut driver, &mut state),
            Ok(StopReason::HandlerYield)
        );
        assert_eq!(state.phase(), SessionPhase::Configuration);
        assert_eq!(driver.buffered_ingress(), 0);
    }

    #[test]
    fn long_fragmentation_corpus_is_deterministic() {
        const RUNS: usize = 10_000;
        let stream = login_route_stream();
        let mut rng = 0x9E37_79B9_u32;
        let mut checksum = 0xCBF2_9CE4_8422_2325u64;

        for run in 0..RUNS {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            let fragment = usize::try_from(rng % 23 + 1).expect("bounded fragment");
            let mut state = SessionState::new();
            let driver = feed_fragmented(&stream, fragment, &mut state).expect("valid route");
            assert_eq!(state.phase(), SessionPhase::Closed, "run={run}");
            assert_eq!(driver.buffered_ingress(), 0, "run={run}");
            assert_eq!(driver.queued_egress(), 0, "run={run}");
            checksum ^= u64::from(rng);
            checksum = checksum.wrapping_mul(0x100_0000_01B3);
        }

        assert_ne!(checksum, 0);
    }
}
