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

/// Compact source-admitted player-ability flags.
///
/// The four independently observable abilities are already a one-byte bitset on the 26.2 wire.
/// Carrying the resolved mask in the target snapshot avoids four booleans plus four branches on
/// every join while keeping invalid high bits unconstructable through the public API.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlayerAbilityFlags(u8);

impl PlayerAbilityFlags {
    /// No ability flags enabled; the selected fresh survival-player profile.
    pub const NONE: Self = Self(0);
    /// Damage immunity enabled.
    pub const INVULNERABLE: Self = Self(1);
    /// Player currently flying.
    pub const FLYING: Self = Self(2);
    /// Player may start flying.
    pub const CAN_FLY: Self = Self(4);
    /// Instant-build semantics enabled.
    pub const INSTABUILD: Self = Self(8);

    /// Combines two source-valid flag sets without exposing raw-bit construction.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn bits(self) -> u8 {
        self.0
    }
}

/// Source-admitted player-abilities state projected during fresh-player bootstrap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerAbilitiesPayload {
    /// Already-resolved compact ability flags.
    pub flags: PlayerAbilityFlags,
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
        let flying = self.flying_speed.to_bits().to_be_bytes();
        let walking = self.walking_speed.to_bits().to_be_bytes();
        let payload = [
            self.flags.bits(),
            flying[0],
            flying[1],
            flying[2],
            flying[3],
            walking[0],
            walking[1],
            walking[2],
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

/// Permission-level entity-event values emitted by vanilla's initial permission publication.
///
/// These are wire event bytes, not a permission-policy abstraction. The product/command owner
/// resolves the semantic permission set; this target layer only projects the reviewed 26.2 event.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionLevelEvent {
    /// Vanilla `LevelBasedPermissionSet.ALL`.
    All = 24,
    /// Vanilla `LevelBasedPermissionSet.MODERATORS`.
    Moderators = 25,
    /// Vanilla `LevelBasedPermissionSet.GAMEMASTERS`.
    Gamemasters = 26,
    /// Vanilla `LevelBasedPermissionSet.ADMINS`.
    Admins = 27,
    /// Vanilla `LevelBasedPermissionSet.OWNERS`.
    Owners = 28,
}

/// Permission entity-event payload: player entity ID followed by the permission event byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PermissionEntityEventPayload {
    /// Runtime entity ID already resolved by the player owner.
    pub entity_id: i32,
    /// Source-admitted permission event identity.
    pub event: PermissionLevelEvent,
}

impl PermissionEntityEventPayload {
    /// Encodes the exact five-byte 26.2 permission entity-event payload.
    ///
    /// # Errors
    ///
    /// Returns the bounded writer error before mutation when all five bytes do not fit.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), PacketCodecError> {
        let entity_id = self.entity_id.to_be_bytes();
        writer.write_bytes(&[
            entity_id[0],
            entity_id[1],
            entity_id[2],
            entity_id[3],
            self.event as u8,
        ])
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
        BootstrapGameEvent, GameEventPayload, HeldSlotPayload, PermissionEntityEventPayload,
        PermissionLevelEvent, PlayerAbilitiesPayload, PlayerAbilityFlags, TickingStatePayload,
        TickingStepPayload,
    };

    #[test]
    fn selected_profile_abilities_payload_matches_vanilla_golden_bytes() {
        let mut writer = PacketWriter::new(9).expect("exact abilities payload bound");
        PlayerAbilitiesPayload {
            flags: PlayerAbilityFlags::NONE,
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
        let all = PlayerAbilityFlags::INVULNERABLE
            .union(PlayerAbilityFlags::FLYING)
            .union(PlayerAbilityFlags::CAN_FLY)
            .union(PlayerAbilityFlags::INSTABUILD);
        let mut writer = PacketWriter::new(9).expect("abilities payload");
        PlayerAbilitiesPayload {
            flags: all,
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
    fn selected_permission_entity_event_matches_vanilla_golden_payload() {
        let mut writer = PacketWriter::new(5).expect("entity-event payload");
        PermissionEntityEventPayload {
            entity_id: 270,
            event: PermissionLevelEvent::All,
        }
        .encode(&mut writer)
        .expect("permission event fits");
        assert_eq!(writer.as_slice(), &[0x00, 0x00, 0x01, 0x0e, 0x18]);
    }

    #[test]
    fn permission_event_values_are_exact_source_constants() {
        assert_eq!(PermissionLevelEvent::All as u8, 24);
        assert_eq!(PermissionLevelEvent::Moderators as u8, 25);
        assert_eq!(PermissionLevelEvent::Gamemasters as u8, 26);
        assert_eq!(PermissionLevelEvent::Admins as u8, 27);
        assert_eq!(PermissionLevelEvent::Owners as u8, 28);
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
            flags: PlayerAbilityFlags::NONE,
            flying_speed: 0.05,
            walking_speed: 0.1,
        }
        .encode(&mut abilities)
        .expect_err("nine-byte payload must not fit");
        assert!(matches!(
            error,
            PacketCodecError::PacketLimitExceeded { .. }
        ));
        assert!(abilities.is_empty());

        let mut permission = PacketWriter::new(4).expect("one byte too small");
        assert!(matches!(
            PermissionEntityEventPayload {
                entity_id: 270,
                event: PermissionLevelEvent::All,
            }
            .encode(&mut permission),
            Err(PacketCodecError::PacketLimitExceeded { .. })
        ));
        assert!(permission.is_empty());

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
