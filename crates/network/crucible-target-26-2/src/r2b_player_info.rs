//! Selected offline initial player-info projection for R2B Minecraft Java 26.2.
//!
//! Source review proves `createPlayerInitializing` selects all eight 26.2 actions. Because that set
//! is the complete eight-value enum, the generic Mojang `EnumSet -> BitSet -> byte[]` path collapses
//! to the exact one-byte action mask `0xff`. Crucible writes that constant directly.
//!
//! The admitted offline profile has an empty property map, no remote chat session and no display
//! component. Those branches therefore emit their zero/false markers directly rather than building
//! property/chat/component objects on every join.

use crucible_packet_core::{PacketCodecError, PacketWriter};

use crate::r2b_login::BootstrapGameMode;

const PLAYER_NAME_MAX_UTF16_UNITS: usize = 16;
const PLAYER_NAME_MAX_UTF8_BYTES: usize = PLAYER_NAME_MAX_UTF16_UNITS * 3;
const INITIAL_ACTION_MASK: u8 = 0xff;

/// One selected offline player-info entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InitialPlayerInfoEntry<'a> {
    /// Canonical UUID bytes in network order (MSB half followed by LSB half).
    pub profile_id: [u8; 16],
    /// Player profile name, bounded by the source-admitted 16 UTF-16-unit law.
    pub name: &'a str,
    /// Current game mode.
    pub game_mode: BootstrapGameMode,
    /// Whether the player is listed in the tab list.
    pub listed: bool,
    /// Initial latency value.
    pub latency: i32,
    /// Initial list ordering value.
    pub list_order: i32,
    /// Whether the player's hat layer is shown.
    pub show_hat: bool,
}

/// Fail-closed selected player-info encoding error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayerInfoEncodeError {
    /// Bounded packet writer failure.
    Codec(PacketCodecError),
    /// Entry count cannot fit the signed Minecraft `VarInt` collection prefix.
    EntryCountDoesNotFitVarInt(usize),
    /// Profile name exceeds the admitted UTF-16-unit bound.
    NameUtf16TooLong { units: usize },
    /// Profile name exceeds the admitted three-bytes-per-unit UTF-8 ceiling.
    NameUtf8TooLong { bytes: usize },
}

impl From<PacketCodecError> for PlayerInfoEncodeError {
    fn from(value: PacketCodecError) -> Self {
        Self::Codec(value)
    }
}

/// Encodes one initial player-info publication body without packet identity.
///
/// An empty slice is meaningful: vanilla sends an all-actions packet with zero entries when there
/// are no existing players to initialize on the joining connection.
///
/// # Errors
///
/// Validates every name/count and the complete payload byte budget before mutation, preserving any
/// already-written packet-ID prefix on failure.
pub(crate) fn encode_initial_player_info(
    entries: &[InitialPlayerInfoEntry<'_>],
    writer: &mut PacketWriter,
) -> Result<(), PlayerInfoEncodeError> {
    let entry_count = i32::try_from(entries.len())
        .map_err(|_| PlayerInfoEncodeError::EntryCountDoesNotFitVarInt(entries.len()))?;

    let mut payload_len = 1_usize
        .checked_add(var_int_len(entry_count))
        .ok_or(PacketCodecError::LengthOverflow)?;
    for entry in entries {
        payload_len = checked_add(payload_len, 16)?;
        payload_len = checked_add(payload_len, player_name_wire_len(entry.name)?)?;
        payload_len = checked_add(payload_len, 1)?; // empty GameProfile property count.
        payload_len = checked_add(payload_len, 1)?; // null chat session.
        payload_len = checked_add(payload_len, var_int_len(entry.game_mode as i32))?;
        payload_len = checked_add(payload_len, 1)?; // listed.
        payload_len = checked_add(payload_len, var_int_len(entry.latency))?;
        payload_len = checked_add(payload_len, 1)?; // null display name.
        payload_len = checked_add(payload_len, var_int_len(entry.list_order))?;
        payload_len = checked_add(payload_len, 1)?; // show hat.
    }
    preflight(writer, payload_len)?;

    writer.write_u8(INITIAL_ACTION_MASK)?;
    writer.write_var_int(entry_count)?;
    for entry in entries {
        writer.write_bytes(&entry.profile_id)?;
        writer.write_string(entry.name, PLAYER_NAME_MAX_UTF16_UNITS)?;
        writer.write_var_int(0)?; // selected offline profile has no GameProfile properties.
        writer.write_bool(false)?; // no RemoteChatSession.Data.
        writer.write_var_int(entry.game_mode as i32)?;
        writer.write_bool(entry.listed)?;
        writer.write_var_int(entry.latency)?;
        writer.write_bool(false)?; // selected default profile has no display component.
        writer.write_var_int(entry.list_order)?;
        writer.write_bool(entry.show_hat)?;
    }
    Ok(())
}

fn player_name_wire_len(value: &str) -> Result<usize, PlayerInfoEncodeError> {
    let units = value.encode_utf16().count();
    if units > PLAYER_NAME_MAX_UTF16_UNITS {
        return Err(PlayerInfoEncodeError::NameUtf16TooLong { units });
    }
    if value.len() > PLAYER_NAME_MAX_UTF8_BYTES {
        return Err(PlayerInfoEncodeError::NameUtf8TooLong { bytes: value.len() });
    }
    let signed = i32::try_from(value.len())
        .map_err(|_| PlayerInfoEncodeError::NameUtf8TooLong { bytes: value.len() })?;
    checked_add(var_int_len(signed), value.len())
}

fn checked_add(left: usize, right: usize) -> Result<usize, PlayerInfoEncodeError> {
    left.checked_add(right).ok_or(PlayerInfoEncodeError::Codec(
        PacketCodecError::LengthOverflow,
    ))
}

fn preflight(writer: &PacketWriter, additional: usize) -> Result<(), PlayerInfoEncodeError> {
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
    use crucible_packet_core::{PacketCodecError, PacketWriter};

    use super::{InitialPlayerInfoEntry, PlayerInfoEncodeError, encode_initial_player_info};
    use crate::r2b_login::BootstrapGameMode;

    const SELF: InitialPlayerInfoEntry<'static> = InitialPlayerInfoEntry {
        profile_id: [
            0x68, 0x20, 0x14, 0xfe, 0xad, 0x63, 0x36, 0x99, 0xaa, 0xda, 0x79, 0xaa, 0x08, 0xd9,
            0x5b, 0x45,
        ],
        name: "Stato16",
        game_mode: BootstrapGameMode::Survival,
        listed: true,
        latency: 0,
        list_order: 0,
        show_hat: true,
    };

    #[test]
    fn empty_existing_player_initialization_matches_capture() {
        let mut writer = PacketWriter::new(2).expect("exact empty player-info payload");
        encode_initial_player_info(&[], &mut writer).expect("empty packet is valid");
        assert_eq!(writer.as_slice(), &[0xff, 0x00]);
    }

    #[test]
    fn selected_self_initialization_matches_exact_r1x_golden_payload() {
        let expected: &[u8] = &[
            0xff, 0x01, 0x68, 0x20, 0x14, 0xfe, 0xad, 0x63, 0x36, 0x99, 0xaa, 0xda, 0x79, 0xaa,
            0x08, 0xd9, 0x5b, 0x45, 0x07, 0x53, 0x74, 0x61, 0x74, 0x6f, 0x31, 0x36, 0x00, 0x00,
            0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
        ];
        let mut writer = PacketWriter::new(expected.len()).expect("exact player-info body");
        encode_initial_player_info(&[SELF], &mut writer).expect("self initialization fits");
        assert_eq!(writer.as_slice(), expected);
    }

    #[test]
    fn all_eight_initial_actions_are_one_static_mask_byte() {
        let mut writer = PacketWriter::new(2).expect("empty initialization");
        encode_initial_player_info(&[], &mut writer).expect("packet fits");
        assert_eq!(writer.as_slice()[0], 0xff);
    }

    #[test]
    fn whole_payload_preflight_preserves_packet_id_prefix() {
        let mut writer = PacketWriter::new(34).expect("one byte short after packet id");
        writer.write_u8(0x46).expect("player-info packet id");
        assert_eq!(
            encode_initial_player_info(&[SELF], &mut writer),
            Err(PlayerInfoEncodeError::Codec(
                PacketCodecError::PacketLimitExceeded {
                    attempted: 35,
                    maximum: 34,
                }
            ))
        );
        assert_eq!(writer.as_slice(), &[0x46]);
    }

    #[test]
    fn overlong_name_fails_before_mutation() {
        let name = "x".repeat(17);
        let entry = InitialPlayerInfoEntry {
            name: &name,
            ..SELF
        };
        let mut writer = PacketWriter::new(128).expect("writer");
        writer.write_u8(0x46).expect("packet id");
        assert!(matches!(
            encode_initial_player_info(&[entry], &mut writer),
            Err(PlayerInfoEncodeError::NameUtf16TooLong { units: 17 })
        ));
        assert_eq!(writer.as_slice(), &[0x46]);
    }
}
