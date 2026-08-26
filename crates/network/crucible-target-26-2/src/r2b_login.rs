//! Allocation-free selected-profile Login payload projection for R2B Minecraft Java 26.2.
//!
//! The semantic owner supplies borrowed dimension identifiers, resolved registry IDs and scalar
//! player/world state. Networking does not construct Mojang `ResourceKey`, `Holder`, `Set`, or
//! `CommonPlayerSpawnInfo` objects. The admitted fresh profile has no last-death location, so that
//! nullable branch is encoded directly as `false`.
//!
//! Every semantic/string/bounds check is completed before the first byte is appended. Once preflight
//! succeeds, all remaining operations are infallible with respect to selected-profile semantics and
//! bounded only by the already-reserved writer capacity.

use crucible_packet_core::{PacketCodecError, PacketWriter};

use crate::r2b_wire::{R2bWireError, write_registry_id};

const IDENTIFIER_MAX_UTF16_UNITS: usize = 32_767;
const IDENTIFIER_MAX_UTF8_BYTES: usize = IDENTIFIER_MAX_UTF16_UNITS * 3;

/// Source-admitted game-mode IDs used by Login and initial player-info publication.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapGameMode {
    /// Survival game mode.
    Survival = 0,
    /// Creative game mode.
    Creative = 1,
    /// Adventure game mode.
    Adventure = 2,
    /// Spectator game mode.
    Spectator = 3,
}

/// Compact semantic flags projected by the selected Login packet.
///
/// Vanilla places these values at separate positions on the wire, but they are independent boolean
/// state in the semantic snapshot. Keeping one typed mask avoids a bool-heavy resident structure and
/// makes every emitted boolean a canonical branch-free `0`/`1` byte.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FreshLoginFlags(u8);

impl FreshLoginFlags {
    /// No optional Login flags enabled.
    pub const NONE: Self = Self(0);
    /// Hardcore-world mode.
    pub const HARDCORE: Self = Self(1 << 0);
    /// Reduced-debug-info game rule.
    pub const REDUCED_DEBUG_INFO: Self = Self(1 << 1);
    /// Client should show its death screen.
    pub const SHOW_DEATH_SCREEN: Self = Self(1 << 2);
    /// Limited-crafting game rule.
    pub const LIMITED_CRAFTING: Self = Self(1 << 3);
    /// Server is using online authentication.
    pub const ONLINE_MODE: Self = Self(1 << 4);
    /// Server enforces secure chat.
    pub const ENFORCES_SECURE_CHAT: Self = Self(1 << 5);

    /// Combines two source-valid semantic flag sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn wire_bool(self, flag: Self) -> u8 {
        u8::from(self.0 & flag.0 != 0)
    }
}

/// Fresh selected-profile `CommonPlayerSpawnInfo` projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreshCommonSpawnInfo<'a> {
    /// Resolved ID in the dimension-type registry. `holderRegistry` emits this raw ID as `VarInt`.
    pub dimension_type_registry_id: i32,
    /// Canonical dimension resource identifier.
    pub dimension: &'a str,
    /// Client-visible hashed world seed.
    pub seed: i64,
    /// Current game mode.
    pub game_mode: BootstrapGameMode,
    /// Previous game mode, or `None` for vanilla's `-1` byte marker.
    pub previous_game_mode: Option<BootstrapGameMode>,
    /// Debug-world flag.
    pub is_debug: bool,
    /// Flat-world flag.
    pub is_flat: bool,
    /// Current portal cooldown.
    pub portal_cooldown: i32,
    /// Dimension sea level.
    pub sea_level: i32,
}

/// Selected fresh/offline Login packet payload without packet identity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FreshLoginPayload<'a> {
    /// Runtime entity ID assigned to the joining player.
    pub player_id: i32,
    /// Compact Login-level boolean state.
    pub flags: FreshLoginFlags,
    /// Ordered dimension identifiers visible to the joining client.
    pub levels: &'a [&'a str],
    /// Server maximum-player display value.
    pub max_players: i32,
    /// Client chunk radius.
    pub chunk_radius: i32,
    /// Simulation distance.
    pub simulation_distance: i32,
    /// Fresh common spawn information.
    pub spawn: FreshCommonSpawnInfo<'a>,
}

/// Fail-closed Login projection error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoginEncodeError {
    /// The bounded packet writer rejected the complete payload.
    Codec(PacketCodecError),
    /// A reusable registry wire constraint failed.
    Wire(R2bWireError),
    /// The level collection cannot be represented by Minecraft's signed `VarInt` count.
    LevelCountDoesNotFitVarInt(usize),
    /// A resource identifier exceeds the admitted Java UTF-16-unit string limit.
    IdentifierUtf16TooLong { units: usize },
    /// A resource identifier exceeds the admitted three-bytes-per-unit encoded ceiling.
    IdentifierUtf8TooLong { bytes: usize },
}

impl From<PacketCodecError> for LoginEncodeError {
    fn from(value: PacketCodecError) -> Self {
        Self::Codec(value)
    }
}

impl From<R2bWireError> for LoginEncodeError {
    fn from(value: R2bWireError) -> Self {
        Self::Wire(value)
    }
}

impl FreshLoginPayload<'_> {
    /// Encodes the exact selected 26.2 Login payload.
    ///
    /// The fresh R2B profile intentionally emits `lastDeathLocation = None`; persisted-player
    /// last-death state belongs to a later admitted profile.
    ///
    /// # Errors
    ///
    /// All semantic/string/count checks and the complete byte-budget preflight occur before writer
    /// mutation. Any error therefore preserves a previously written packet-ID prefix byte-for-byte.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), LoginEncodeError> {
        let level_count = i32::try_from(self.levels.len())
            .map_err(|_| LoginEncodeError::LevelCountDoesNotFitVarInt(self.levels.len()))?;

        if self.spawn.dimension_type_registry_id < 0 {
            return Err(
                R2bWireError::NegativeRegistryId(self.spawn.dimension_type_registry_id).into(),
            );
        }

        let mut payload_len = 4_usize + 1 + var_int_len(level_count);
        for level in self.levels {
            payload_len = checked_add(payload_len, identifier_wire_len(level)?)?;
        }
        payload_len = checked_add(payload_len, var_int_len(self.max_players))?;
        payload_len = checked_add(payload_len, var_int_len(self.chunk_radius))?;
        payload_len = checked_add(payload_len, var_int_len(self.simulation_distance))?;
        payload_len = checked_add(payload_len, 3)?; // reduced debug, death screen, limited crafting.
        payload_len = checked_add(
            payload_len,
            var_int_len(self.spawn.dimension_type_registry_id),
        )?;
        payload_len = checked_add(payload_len, identifier_wire_len(self.spawn.dimension)?)?;
        payload_len = checked_add(payload_len, 8 + 2 + 3)?; // seed, game modes, debug/flat/last-death marker.
        payload_len = checked_add(payload_len, var_int_len(self.spawn.portal_cooldown))?;
        payload_len = checked_add(payload_len, var_int_len(self.spawn.sea_level))?;
        payload_len = checked_add(payload_len, 2)?; // online mode + secure chat.
        preflight(writer, payload_len)?;

        writer.write_i32(self.player_id)?;
        writer.write_u8(self.flags.wire_bool(FreshLoginFlags::HARDCORE))?;
        writer.write_var_int(level_count)?;
        for level in self.levels {
            writer.write_string(level, IDENTIFIER_MAX_UTF16_UNITS)?;
        }
        writer.write_var_int(self.max_players)?;
        writer.write_var_int(self.chunk_radius)?;
        writer.write_var_int(self.simulation_distance)?;
        writer.write_u8(self.flags.wire_bool(FreshLoginFlags::REDUCED_DEBUG_INFO))?;
        writer.write_u8(self.flags.wire_bool(FreshLoginFlags::SHOW_DEATH_SCREEN))?;
        writer.write_u8(self.flags.wire_bool(FreshLoginFlags::LIMITED_CRAFTING))?;

        write_registry_id(writer, self.spawn.dimension_type_registry_id)?;
        writer.write_string(self.spawn.dimension, IDENTIFIER_MAX_UTF16_UNITS)?;
        writer.write_i64(self.spawn.seed)?;
        writer.write_u8(self.spawn.game_mode as u8)?;
        writer.write_u8(
            self.spawn
                .previous_game_mode
                .map_or(0xff, |mode| mode as u8),
        )?;
        writer.write_bool(self.spawn.is_debug)?;
        writer.write_bool(self.spawn.is_flat)?;
        writer.write_u8(0)?; // selected fresh profile: no last-death GlobalPos.
        writer.write_var_int(self.spawn.portal_cooldown)?;
        writer.write_var_int(self.spawn.sea_level)?;
        writer.write_u8(self.flags.wire_bool(FreshLoginFlags::ONLINE_MODE))?;
        writer.write_u8(self.flags.wire_bool(FreshLoginFlags::ENFORCES_SECURE_CHAT))?;
        Ok(())
    }
}

fn identifier_wire_len(value: &str) -> Result<usize, LoginEncodeError> {
    let units = value.encode_utf16().count();
    if units > IDENTIFIER_MAX_UTF16_UNITS {
        return Err(LoginEncodeError::IdentifierUtf16TooLong { units });
    }
    if value.len() > IDENTIFIER_MAX_UTF8_BYTES {
        return Err(LoginEncodeError::IdentifierUtf8TooLong { bytes: value.len() });
    }
    let signed = i32::try_from(value.len())
        .map_err(|_| LoginEncodeError::IdentifierUtf8TooLong { bytes: value.len() })?;
    checked_add(var_int_len(signed), value.len())
}

fn checked_add(left: usize, right: usize) -> Result<usize, LoginEncodeError> {
    left.checked_add(right)
        .ok_or(LoginEncodeError::Codec(PacketCodecError::LengthOverflow))
}

fn preflight(writer: &PacketWriter, additional: usize) -> Result<(), LoginEncodeError> {
    if additional <= writer.remaining_capacity() {
        return Ok(());
    }
    let attempted = writer
        .len()
        .checked_add(additional)
        .ok_or(PacketCodecError::LengthOverflow)?;
    let maximum = writer
        .len()
        .checked_add(writer.remaining_capacity())
        .ok_or(PacketCodecError::LengthOverflow)?;
    Err(PacketCodecError::PacketLimitExceeded { attempted, maximum }.into())
}

const fn var_int_len(value: i32) -> usize {
    let mut remaining = value.cast_unsigned();
    let mut length = 1_usize;
    while remaining & !0x7f != 0 {
        remaining >>= 7;
        length += 1;
    }
    length
}

#[cfg(test)]
mod tests {
    use crucible_packet_core::PacketWriter;

    use super::{
        BootstrapGameMode, FreshCommonSpawnInfo, FreshLoginFlags, FreshLoginPayload,
        LoginEncodeError,
    };

    const LEVELS: [&str; 3] = [
        "minecraft:overworld",
        "minecraft:the_nether",
        "minecraft:the_end",
    ];

    const SELECTED: FreshLoginPayload<'static> = FreshLoginPayload {
        player_id: 270,
        flags: FreshLoginFlags::SHOW_DEATH_SCREEN,
        levels: &LEVELS,
        max_players: 20,
        chunk_radius: 10,
        simulation_distance: 10,
        spawn: FreshCommonSpawnInfo {
            dimension_type_registry_id: 0,
            dimension: "minecraft:overworld",
            seed: 0x1439_77a8_ee42_e04a,
            game_mode: BootstrapGameMode::Survival,
            previous_game_mode: None,
            is_debug: false,
            is_flat: false,
            portal_cooldown: 0,
            sea_level: 63,
        },
    };

    #[test]
    fn selected_login_matches_exact_r1x_golden_payload() {
        let expected: &[u8] = &[
            0x00, 0x00, 0x01, 0x0e, 0x00, 0x03, 0x13, 0x6d, 0x69, 0x6e, 0x65, 0x63, 0x72, 0x61,
            0x66, 0x74, 0x3a, 0x6f, 0x76, 0x65, 0x72, 0x77, 0x6f, 0x72, 0x6c, 0x64, 0x14, 0x6d,
            0x69, 0x6e, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x3a, 0x74, 0x68, 0x65, 0x5f, 0x6e,
            0x65, 0x74, 0x68, 0x65, 0x72, 0x11, 0x6d, 0x69, 0x6e, 0x65, 0x63, 0x72, 0x61, 0x66,
            0x74, 0x3a, 0x74, 0x68, 0x65, 0x5f, 0x65, 0x6e, 0x64, 0x14, 0x0a, 0x0a, 0x00, 0x01,
            0x00, 0x00, 0x13, 0x6d, 0x69, 0x6e, 0x65, 0x63, 0x72, 0x61, 0x66, 0x74, 0x3a, 0x6f,
            0x76, 0x65, 0x72, 0x77, 0x6f, 0x72, 0x6c, 0x64, 0x14, 0x39, 0x77, 0xa8, 0xee, 0x42,
            0xe0, 0x4a, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x3f, 0x00, 0x00,
        ];
        let mut writer = PacketWriter::new(expected.len()).expect("exact Login payload bound");
        SELECTED.encode(&mut writer).expect("selected Login fits");
        assert_eq!(writer.as_slice(), expected);
    }

    #[test]
    fn complete_payload_preflight_preserves_existing_packet_id_prefix() {
        let mut writer = PacketWriter::new(109).expect("one byte short after packet id");
        writer.write_u8(0x31).expect("Login packet id prefix");
        let error = SELECTED
            .encode(&mut writer)
            .expect_err("109-byte payload cannot fit after one prefix byte");
        assert_eq!(
            error,
            LoginEncodeError::Codec(
                crucible_packet_core::PacketCodecError::PacketLimitExceeded {
                    attempted: 110,
                    maximum: 109,
                }
            )
        );
        assert_eq!(writer.as_slice(), &[0x31]);
    }

    #[test]
    fn invalid_semantics_fail_before_any_writer_mutation() {
        let mut writer = PacketWriter::new(256).expect("writer");
        writer.write_u8(0x31).expect("existing packet id");

        let negative_dimension = FreshLoginPayload {
            spawn: FreshCommonSpawnInfo {
                dimension_type_registry_id: -1,
                ..SELECTED.spawn
            },
            ..SELECTED
        };
        assert!(matches!(
            negative_dimension.encode(&mut writer),
            Err(LoginEncodeError::Wire(_))
        ));
        assert_eq!(writer.as_slice(), &[0x31]);

        let long_dimension = "x".repeat(32_768);
        let invalid_identifier = FreshLoginPayload {
            spawn: FreshCommonSpawnInfo {
                dimension: &long_dimension,
                ..SELECTED.spawn
            },
            ..SELECTED
        };
        assert!(matches!(
            invalid_identifier.encode(&mut writer),
            Err(LoginEncodeError::IdentifierUtf16TooLong { .. })
        ));
        assert_eq!(writer.as_slice(), &[0x31]);
    }

    #[test]
    fn login_flags_compact_independent_boolean_state() {
        let all = FreshLoginFlags::HARDCORE
            .union(FreshLoginFlags::REDUCED_DEBUG_INFO)
            .union(FreshLoginFlags::SHOW_DEATH_SCREEN)
            .union(FreshLoginFlags::LIMITED_CRAFTING)
            .union(FreshLoginFlags::ONLINE_MODE)
            .union(FreshLoginFlags::ENFORCES_SECURE_CHAT);
        assert_ne!(all, FreshLoginFlags::NONE);
    }

    #[test]
    fn every_game_mode_uses_the_source_literal_id() {
        assert_eq!(BootstrapGameMode::Survival as u8, 0);
        assert_eq!(BootstrapGameMode::Creative as u8, 1);
        assert_eq!(BootstrapGameMode::Adventure as u8, 2);
        assert_eq!(BootstrapGameMode::Spectator as u8, 3);
    }
}
