//! Fresh empty-inventory specialization for R2B Minecraft Java 26.2.
//!
//! The selected fresh `InventoryMenu` snapshot contains only empty `ItemStack`s, and source review
//! proves the empty optional-stack encoding is exactly `VarInt(0)`, i.e. one zero byte. Crucible
//! therefore carries only the resolved container ID, state ID and menu slot count. It never builds a
//! Mojang menu object, per-slot `ItemStack` objects, or a temporary `Vec<ItemStack>` on the join path.
//!
//! Menu layout policy stays outside networking: the inventory/menu owner supplies `slot_count`.
//! Non-empty or persisted stacks are rejected by architecture rather than silently entering this
//! fresh-only encoder; those profiles require their own admitted general ItemStack projection.

use crucible_packet_core::{PacketCodecError, PacketWriter};

const ZERO_CHUNK: [u8; 64] = [0; 64];

/// Resolved fresh full-container snapshot consisting entirely of empty stacks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FreshEmptyInventoryPayload {
    /// Container/menu ID resolved by the inventory owner.
    pub container_id: i32,
    /// Current container state ID.
    pub state_id: i32,
    /// Number of menu slots in the resolved layout. Carried item is encoded separately as empty.
    pub slot_count: usize,
}

/// Fail-closed fresh inventory encoding error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InventoryEncodeError {
    /// Bounded packet writer failure.
    Codec(PacketCodecError),
    /// Menu slot count cannot fit Minecraft's signed VarInt list count.
    SlotCountDoesNotFitVarInt(usize),
}

impl From<PacketCodecError> for InventoryEncodeError {
    fn from(value: PacketCodecError) -> Self {
        Self::Codec(value)
    }
}

impl FreshEmptyInventoryPayload {
    /// Encodes `containerId + stateId + slotCount + empty slots + empty carried stack`.
    ///
    /// The empty stack markers are copied from one static zero block in bounded chunks, avoiding both
    /// per-slot allocation and 47 independent bounds checks on the selected 46-slot inventory menu.
    ///
    /// # Errors
    ///
    /// Validates count representability and complete payload capacity before any writer mutation.
    pub fn encode(self, writer: &mut PacketWriter) -> Result<(), InventoryEncodeError> {
        let slot_count = i32::try_from(self.slot_count)
            .map_err(|_| InventoryEncodeError::SlotCountDoesNotFitVarInt(self.slot_count))?;
        let empty_markers = self
            .slot_count
            .checked_add(1)
            .ok_or(PacketCodecError::LengthOverflow)?; // + carried item.
        let payload_len = var_int_len(self.container_id)
            .checked_add(var_int_len(self.state_id))
            .and_then(|value| value.checked_add(var_int_len(slot_count)))
            .and_then(|value| value.checked_add(empty_markers))
            .ok_or(PacketCodecError::LengthOverflow)?;
        preflight(writer, payload_len)?;

        writer.write_var_int(self.container_id)?;
        writer.write_var_int(self.state_id)?;
        writer.write_var_int(slot_count)?;
        write_zeroes(writer, empty_markers)?;
        Ok(())
    }
}

fn write_zeroes(writer: &mut PacketWriter, mut count: usize) -> Result<(), PacketCodecError> {
    while count != 0 {
        let chunk = count.min(ZERO_CHUNK.len());
        writer.write_bytes(&ZERO_CHUNK[..chunk])?;
        count -= chunk;
    }
    Ok(())
}

fn preflight(writer: &PacketWriter, additional: usize) -> Result<(), InventoryEncodeError> {
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
    let mut remaining = value as u32;
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

    use super::{FreshEmptyInventoryPayload, InventoryEncodeError};

    const SELECTED: FreshEmptyInventoryPayload = FreshEmptyInventoryPayload {
        container_id: 0,
        state_id: 1,
        slot_count: 46,
    };

    #[test]
    fn selected_fresh_inventory_matches_exact_r1x_golden_payload() {
        let mut writer = PacketWriter::new(50).expect("exact container-content payload bound");
        SELECTED
            .encode(&mut writer)
            .expect("fresh empty inventory fits");

        let mut expected = Vec::with_capacity(50);
        expected.extend_from_slice(&[0x00, 0x01, 0x2e]);
        expected.extend_from_slice(&[0; 47]); // 46 slots plus carried stack.
        assert_eq!(writer.as_slice(), expected);
    }

    #[test]
    fn menu_layout_is_owner_supplied_not_hardcoded_to_46_slots() {
        let mut writer = PacketWriter::new(8).expect("small empty menu");
        FreshEmptyInventoryPayload {
            container_id: 7,
            state_id: 9,
            slot_count: 3,
        }
        .encode(&mut writer)
        .expect("three-slot menu fits");
        assert_eq!(
            writer.as_slice(),
            &[0x07, 0x09, 0x03, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn whole_payload_preflight_preserves_packet_id_prefix() {
        let mut writer = PacketWriter::new(50).expect("one byte short after packet id");
        writer.write_u8(0x12).expect("container-content packet id");
        assert_eq!(
            SELECTED.encode(&mut writer),
            Err(InventoryEncodeError::Codec(
                PacketCodecError::PacketLimitExceeded {
                    attempted: 51,
                    maximum: 50,
                }
            ))
        );
        assert_eq!(writer.as_slice(), &[0x12]);
    }

    #[test]
    fn large_slot_count_fails_before_mutation() {
        if usize::BITS <= 32 {
            return;
        }
        let mut writer = PacketWriter::new(8).expect("writer");
        writer.write_u8(0x12).expect("packet id");
        let count = (i32::MAX as usize) + 1;
        assert_eq!(
            FreshEmptyInventoryPayload {
                container_id: 0,
                state_id: 0,
                slot_count: count,
            }
            .encode(&mut writer),
            Err(InventoryEncodeError::SlotCountDoesNotFitVarInt(count))
        );
        assert_eq!(writer.as_slice(), &[0x12]);
    }
}
