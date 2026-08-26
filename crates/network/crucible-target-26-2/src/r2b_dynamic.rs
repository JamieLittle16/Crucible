//! Allocation-free scalar payload codecs for the R2B Minecraft Java 26.2 bootstrap.
//!
//! Packet identity is deliberately absent from this module. The eventual R2B assembler prefixes
//! source-generated compile-time packet IDs; these functions encode only vanilla-observable packet
//! fields into a caller-owned bounded [`PacketWriter`]. Keeping identity and payload projection
//! separate lets semantic codecs remain small, testable, reusable and free of runtime registries.
//!
//! Fixed-size payloads are assembled in stack arrays and appended with one `write_bytes` call. That
//! gives each payload one packet-budget check and whole-payload transactional mutation without a
//! temporary heap allocation. Variable-width payloads below consist of one `VarInt`, whose existing
//! writer primitive is already transactional.

use crucible_packet_core::{PacketCodecError, PacketWriter};

/// Source-admitted player-abilities state projected during fresh-player bootstrap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerAbilitiesPayload {
    /// Whether damage immunity is enabled.
    pub invulnerable: bool,
    /// Whether the player is currently flying.
    pub is_flying: bool,
    /// Whether the player may start flying.
    pub can_fly: bool,
    /// Whether instant-build semantics are enabled.
    pub instabuild: bool,
    /// Vanilla flying-speed scalar.
    pub flying_speed: f32,
    /// Vanilla walking-speed scalar.
    pub walking_speed: f32,
}

impl PlayerAbilitiesPayload {
    /// Encodes the exact 26.2 abilities payload: flags byte, flying speed, walking speed.
    ///
    /// # Errors
    ///
    /// Returns the bounded writer error before mutation when the complete nine-byte payload does
    /// not fit.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        let mut flags = 0_u8;
        if self.invulnerable {
            flags |= 1;
        }
        if self.is_flying {
            flags |= 2;
        }
        if self.can_fly {
            flags |= 4;
        }
        if self.instabuild {
            flags |= 8;
        }

        let flying = self.flying_speed.to_bits().to_be_bytes();
        let walking = self.walking_speed.to_bits().to_be_bytes();
        let payload = [
            flags, flying[0], flying[1], flying[2], flying[3], walking[0], walking[1], walking[2],
            walking[3],
        ];
        writer.write_bytes(&payload)
    }
}

/// One source-admitted initial held-slot value.
///
/// This is a resolved wire value, not a Mojang inventory object. Semantic slot-range policy remains
/// with the player/inventory owner; the packet codec itself is exactly one signed Minecraft
/// `VarInt` in 26.2.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeldSlotPayload(pub i32);

impl HeldSlotPayload {
    /// Encodes the slot as the packet's sole `VarInt` field.
    ///
    /// # Errors
    ///
    /// Propagates the bounded writer error without partial field mutation.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        writer.write_var_int(self.0)
    }
}

/// The R2B-selected game events reachable during initial level publication.
///
/// Restricting this enum to the reviewed bootstrap route prevents an unrelated gameplay event from
/// entering R2B merely because it shares the same generic packet type.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapGameEvent {
    /// Conditional weather branch: begin rain.
    StartRaining = 1,
    /// Conditional weather branch: rain intensity changed.
    RainLevelChange = 7,
    /// Conditional weather branch: thunder intensity changed.
    ThunderLevelChange = 8,
    /// Mandatory fresh-player level-info boundary before tick-rate synchronization.
    LevelChunksLoadStart = 13,
}

/// One bootstrap game-event payload.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GameEventPayload {
    /// Source-admitted event identity.
    pub event: BootstrapGameEvent,
    /// Event parameter interpreted by the vanilla client according to `event`.
    pub parameter: f32,
}

impl GameEventPayload {
    /// Encodes one event byte followed by one IEEE-754 binary32 parameter.
    ///
    /// # Errors
    ///
    /// Returns the bounded writer error before mutation when all five bytes do not fit.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        let parameter = self.parameter.to_bits().to_be_bytes();
        writer.write_bytes(&[
            self.event as u8,
            parameter[0],
            parameter[1],
            parameter[2],
            parameter[3],
        ])
    }
}

/// Tick-rate state sent to every joining player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TickingStatePayload {
    /// Current server tick rate.
    pub tick_rate: f32,
    /// Whether ticking is frozen.
    pub is_frozen: bool,
}

impl TickingStatePayload {
    /// Encodes tick rate followed by the canonical boolean freeze byte.
    ///
    /// # Errors
    ///
    /// Returns the bounded writer error before mutation when all five bytes do not fit.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        let tick_rate = self.tick_rate.to_bits().to_be_bytes();
        writer.write_bytes(&[
            tick_rate[0],
            tick_rate[1],
            tick_rate[2],
            tick_rate[3],
            u8::from(self.is_frozen),
        ])
    }
}

/// Remaining frozen tick steps sent alongside [`TickingStatePayload`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TickingStepPayload(pub i32);

impl TickingStepPayload {
    /// Encodes the packet's sole signed Minecraft `VarInt` field.
    ///
    /// # Errors
    ///
    /// Propagates the bounded writer error without partial field mutation.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        writer.write_var_int(self.0)
    }
}

#[cfg(test)]
mod tests {
    use crucible_packet_core::{PacketCodecError, PacketWriter};

    use super::{
        BootstrapGameEvent, GameEventPayload, HeldSlotPayload, PlayerAbilitiesPayload,
        TickingStatePayload, TickingStepPayload,
    };

    #[test]
    fn selected_profile_abilities_payload_matches_vanilla_golden_bytes() {
        let mut writer = PacketWriter::new(9).expect("exact abilities payload bound");
        PlayerAbilitiesPayload {
            invulnerable: false,
            is_flying: false,
            can_fly: false,
            instabuild: false,
            flying_speed: 0.05,
            walking_speed: 0.1,
        }
        .encode(&mut writer)
        .expect("selected abilities fit");

        assert_eq!(
            writer.as_slice(),
            &[0x00, 0x3d, 0x4c, 0xcc, 0xcd, 0x3d, 0xcc, 0xcc, 0xcd]
        );
    }

    #[test]
    fn all_abilities_flags_use_the_source_bit_assignments() {
        let mut writer = PacketWriter::new(9).expect("abilities payload");
        PlayerAbilitiesPayload {
            invulnerable: true,
            is_flying: true,
            can_fly: true,
            instabuild: true,
            flying_speed: f32::from_bits(0x8000_0000),
            walking_speed: f32::from_bits(0x7fc0_1234),
        }
        .encode(&mut writer)
        .expect("payload fits");

        assert_eq!(writer.as_slice()[0], 0x0f);
        assert_eq!(&writer.as_slice()[1..5], &0x8000_0000_u32.to_be_bytes());
        assert_eq!(&writer.as_slice()[5..9], &0x7fc0_1234_u32.to_be_bytes());
    }

    #[test]
    fn held_slot_and_ticking_step_are_single_varints() {
        let mut held = PacketWriter::new(5).expect("held-slot payload");
        HeldSlotPayload(0).encode(&mut held).expect("slot zero");
        assert_eq!(held.as_slice(), &[0x00]);

        let mut step = PacketWriter::new(5).expect("tick-step payload");
        TickingStepPayload(0).encode(&mut step).expect("zero steps");
        assert_eq!(step.as_slice(), &[0x00]);
    }

    #[test]
    fn mandatory_load_start_event_matches_vanilla_golden_payload() {
        let mut writer = PacketWriter::new(5).expect("game-event payload");
        GameEventPayload {
            event: BootstrapGameEvent::LevelChunksLoadStart,
            parameter: 0.0,
        }
        .encode(&mut writer)
        .expect("event fits");
        assert_eq!(writer.as_slice(), &[0x0d, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn weather_event_ids_are_exact_source_constants() {
        assert_eq!(BootstrapGameEvent::StartRaining as u8, 1);
        assert_eq!(BootstrapGameEvent::RainLevelChange as u8, 7);
        assert_eq!(BootstrapGameEvent::ThunderLevelChange as u8, 8);
        assert_eq!(BootstrapGameEvent::LevelChunksLoadStart as u8, 13);
    }

    #[test]
    fn selected_ticking_state_matches_vanilla_golden_payload() {
        let mut writer = PacketWriter::new(5).expect("ticking-state payload");
        TickingStatePayload {
            tick_rate: 20.0,
            is_frozen: false,
        }
        .encode(&mut writer)
        .expect("state fits");
        assert_eq!(writer.as_slice(), &[0x41, 0xa0, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn fixed_size_payloads_fail_as_one_transaction() {
        let mut abilities = PacketWriter::new(8).expect("one byte too small");
        let error = PlayerAbilitiesPayload {
            invulnerable: false,
            is_flying: false,
            can_fly: false,
            instabuild: false,
            flying_speed: 0.05,
            walking_speed: 0.1,
        }
        .encode(&mut abilities)
        .expect_err("nine-byte payload must not fit");
        assert!(matches!(error, PacketCodecError::PacketLimitExceeded { .. }));
        assert!(abilities.is_empty());

        let mut event = PacketWriter::new(4).expect("one byte too small");
        assert!(matches!(
            GameEventPayload {
                event: BootstrapGameEvent::LevelChunksLoadStart,
                parameter: 0.0,
            }
            .encode(&mut event),
            Err(PacketCodecError::PacketLimitExceeded { .. })
        ));
        assert!(event.is_empty());

        let mut ticking = PacketWriter::new(4).expect("one byte too small");
        assert!(matches!(
            TickingStatePayload {
                tick_rate: 20.0,
                is_frozen: false,
            }
            .encode(&mut ticking),
            Err(PacketCodecError::PacketLimitExceeded { .. })
        ));
        assert!(ticking.is_empty());
    }
}
