//! Allocation-free full clock synchronization for the R2B Minecraft Java 26.2 bootstrap.
//!
//! Vanilla constructs a temporary `HashMap<Holder<WorldClock>, ClockNetworkState>` for the initial
//! packet. Map iteration order is not a semantic clock property, so Crucible instead receives a
//! borrowed sequence in strictly increasing resolved registry-ID order. That order is deterministic,
//! proves key uniqueness in one linear pass and requires no map/sort/allocation in networking.
//!
//! `WorldClock.STREAM_CODEC` uses `holderRegistry`, whose outbound path is the plain registry-ID
//! `VarInt` codec (not the `id + 1` marker used by the generic direct/reference holder codec).

use crucible_packet_core::{PacketCodecError, PacketWriter};

use crate::r2b_wire::{R2bWireError, write_bounded_collection_len, write_registry_id};

const CLOCK_ENTRY_FIXED_BYTES: usize = 4 + 4;
const MAX_CLOCK_UPDATES: usize = i32::MAX as usize;

/// One already-resolved client-visible clock entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockUpdate {
    /// Resolved `WORLD_CLOCK` registry ID. Entries must be strictly increasing by this field.
    pub registry_id: i32,
    /// Total ticks encoded as Minecraft `VarLong`.
    pub total_ticks: i64,
    /// Fractional tick progress.
    pub partial_tick: f32,
    /// Effective client rate; semantic ownership has already applied pause/global-time policy.
    pub rate: f32,
}

/// Borrowed full clock snapshot used by `ClientboundSetTimePacket`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockFullSyncPayload<'a> {
    /// Server game time written as a fixed network-order `i64`.
    pub game_time: i64,
    /// Unique entries in strictly increasing resolved registry-ID order.
    pub updates: &'a [ClockUpdate],
}

/// Fail-closed clock projection error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockProjectionError {
    /// Reusable R2B registry/count or bounded writer failure.
    Wire(R2bWireError),
    /// Registry IDs were not strictly increasing, so the slice is not the canonical unique map view.
    NonCanonicalRegistryOrder {
        /// Previous registry ID.
        previous: i32,
        /// Current registry ID that did not increase.
        current: i32,
    },
}

impl From<R2bWireError> for ClockProjectionError {
    fn from(value: R2bWireError) -> Self {
        Self::Wire(value)
    }
}

impl From<PacketCodecError> for ClockProjectionError {
    fn from(value: PacketCodecError) -> Self {
        Self::Wire(R2bWireError::Codec(value))
    }
}

impl ClockFullSyncPayload<'_> {
    /// Encodes the exact 26.2 full clock packet payload without constructing a map.
    ///
    /// The complete payload is preflighted before the first writer mutation. Registry order is also
    /// validated before mutation, so malformed semantic input cannot leave a partial packet body.
    ///
    /// # Errors
    ///
    /// Rejects negative registry IDs, duplicate/out-of-order IDs, impossible collection counts or a
    /// bounded writer that cannot contain the complete encoded payload.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), ClockProjectionError> {
        let payload_len = encoded_len(self.updates)?;
        preflight(writer, payload_len)?;

        writer.write_i64(self.game_time)?;
        write_bounded_collection_len(writer, self.updates.len(), MAX_CLOCK_UPDATES)?;
        for update in self.updates {
            write_registry_id(writer, update.registry_id)?;
            writer.write_var_long(update.total_ticks)?;
            writer.write_f32(update.partial_tick)?;
            writer.write_f32(update.rate)?;
        }
        Ok(())
    }
}

fn encoded_len(updates: &[ClockUpdate]) -> Result<usize, ClockProjectionError> {
    if updates.len() > MAX_CLOCK_UPDATES {
        return Err(R2bWireError::CollectionTooLarge {
            length: updates.len(),
            maximum: MAX_CLOCK_UPDATES,
        }
        .into());
    }

    let mut length = 8_usize
        .checked_add(var_int_len_nonnegative(updates.len()))
        .ok_or(PacketCodecError::LengthOverflow)?;
    let mut previous: Option<i32> = None;

    for update in updates {
        if update.registry_id < 0 {
            return Err(R2bWireError::NegativeRegistryId(update.registry_id).into());
        }
        if let Some(previous) = previous
            && update.registry_id <= previous
        {
            return Err(ClockProjectionError::NonCanonicalRegistryOrder {
                previous,
                current: update.registry_id,
            });
        }
        previous = Some(update.registry_id);

        length = length
            .checked_add(var_int_len_i32(update.registry_id))
            .and_then(|value| value.checked_add(var_long_len(update.total_ticks)))
            .and_then(|value| value.checked_add(CLOCK_ENTRY_FIXED_BYTES))
            .ok_or(PacketCodecError::LengthOverflow)?;
    }
    Ok(length)
}

fn preflight(writer: &PacketWriter, additional: usize) -> Result<(), PacketCodecError> {
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
    Err(PacketCodecError::PacketLimitExceeded { attempted, maximum })
}

fn var_int_len_nonnegative(value: usize) -> usize {
    let value = u64::try_from(value).expect("usize fits u64 on supported targets");
    unsigned_var_len(value)
}

fn var_int_len_i32(value: i32) -> usize {
    unsigned_var_len(u64::from(value.cast_unsigned()))
}

fn var_long_len(value: i64) -> usize {
    unsigned_var_len(value.cast_unsigned())
}

fn unsigned_var_len(value: u64) -> usize {
    let significant_bits = u64::BITS - value.leading_zeros();
    usize::try_from(significant_bits.max(1).div_ceil(7))
        .expect("variable integer length fits usize")
}

#[cfg(test)]
mod tests {
    use crucible_packet_core::{PacketCodecError, PacketWriter};

    use super::{ClockFullSyncPayload, ClockProjectionError, ClockUpdate};
    use crate::r2b_wire::R2bWireError;

    const UPDATES: [ClockUpdate; 2] = [
        ClockUpdate {
            registry_id: 0,
            total_ticks: 300,
            partial_tick: 0.5,
            rate: 1.0,
        },
        ClockUpdate {
            registry_id: 1,
            total_ticks: 127,
            partial_tick: 0.25,
            rate: 0.0,
        },
    ];

    #[test]
    fn full_sync_matches_source_field_and_map_entry_order() {
        let payload = ClockFullSyncPayload {
            game_time: 42,
            updates: &UPDATES,
        };
        let mut writer = PacketWriter::new(30).expect("exact synthetic clock payload bound");
        payload.encode(&mut writer).expect("synthetic payload fits");

        assert_eq!(
            writer.as_slice(),
            &[
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2a, // game time
                0x02, // map count
                0x00, // plain WORLD_CLOCK registry id 0
                0xac, 0x02, // total ticks 300
                0x3f, 0x00, 0x00, 0x00, // partial 0.5
                0x3f, 0x80, 0x00, 0x00, // rate 1.0
                0x01, // plain WORLD_CLOCK registry id 1
                0x7f, // total ticks 127
                0x3e, 0x80, 0x00, 0x00, // partial 0.25
                0x00, 0x00, 0x00, 0x00, // effective paused rate 0.0
            ]
        );
    }

    #[test]
    fn empty_clock_map_is_valid_and_exact() {
        let mut writer = PacketWriter::new(9).expect("game time plus zero count");
        ClockFullSyncPayload {
            game_time: -1,
            updates: &[],
        }
        .encode(&mut writer)
        .expect("empty map is wire-valid");

        assert_eq!(
            writer.as_slice(),
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00]
        );
    }

    #[test]
    fn order_and_duplicate_validation_happens_before_mutation() {
        let noncanonical = [
            ClockUpdate {
                registry_id: 3,
                ..UPDATES[0]
            },
            ClockUpdate {
                registry_id: 3,
                ..UPDATES[1]
            },
        ];
        let mut writer = PacketWriter::new(64).expect("writer");
        writer.write_u8(0x55).expect("existing prefix");

        assert_eq!(
            ClockFullSyncPayload {
                game_time: 42,
                updates: &noncanonical,
            }
            .encode(&mut writer),
            Err(ClockProjectionError::NonCanonicalRegistryOrder {
                previous: 3,
                current: 3,
            })
        );
        assert_eq!(writer.as_slice(), &[0x55]);
    }

    #[test]
    fn negative_registry_id_fails_before_mutation() {
        let update = [ClockUpdate {
            registry_id: -1,
            ..UPDATES[0]
        }];
        let mut writer = PacketWriter::new(64).expect("writer");
        assert_eq!(
            ClockFullSyncPayload {
                game_time: 0,
                updates: &update,
            }
            .encode(&mut writer),
            Err(ClockProjectionError::Wire(
                R2bWireError::NegativeRegistryId(-1)
            ))
        );
        assert!(writer.is_empty());
    }

    #[test]
    fn whole_payload_bound_rejection_preserves_existing_prefix() {
        let mut writer = PacketWriter::new(30).expect("one byte short after prefix");
        writer.write_u8(0x55).expect("existing prefix");

        let error = ClockFullSyncPayload {
            game_time: 42,
            updates: &UPDATES,
        }
        .encode(&mut writer)
        .expect_err("30-byte payload cannot fit in remaining 29 bytes");

        assert_eq!(
            error,
            ClockProjectionError::Wire(R2bWireError::Codec(
                PacketCodecError::PacketLimitExceeded {
                    attempted: 31,
                    maximum: 30,
                }
            ))
        );
        assert_eq!(writer.as_slice(), &[0x55]);
    }
}
