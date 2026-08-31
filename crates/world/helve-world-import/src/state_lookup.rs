//! Allocation-free lookup of persisted Minecraft 26.2 block-state identities.
//!
//! The generated table is a cold import artifact. A saved state is hashed without constructing a
//! canonical string, mapped to exactly one generated candidate, then verified structurally before
//! the existing dense `BlockStateId` is returned. Hashes are indexing only; exact structured
//! equality remains the semantic authority.

use helve_generated::{BLOCK_STATE_COUNT, BlockStateId};

use crate::{
    generated_state_lookup as generated,
    stored_blocks::{BlockProperty, BlockStateResolver},
};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const SPLITMIX_INCREMENT: u64 = 0x9e37_79b9_7f4a_7c15;
const SPLITMIX_MIX1: u64 = 0xbf58_476d_1ce4_e5b9;
const SPLITMIX_MIX2: u64 = 0x94d0_49bb_1331_11eb;
const STATE_NAME_MASK: u32 = (1 << 11) - 1;
const STATE_PROPERTY_START_MASK: u32 = (1 << 18) - 1;
const PROPERTY_ID_MASK: u16 = (1 << 9) - 1;

const _: () = assert!(generated::STORED_STATE_LOOKUP_BYTES > 0);
const _: () = assert!(generated::STORED_STATE_LOOKUP_COUNT == BLOCK_STATE_COUNT);
const _: () = assert!(generated::STORED_STATE_BUCKET_COUNT.is_power_of_two());
const _: () = assert!(generated::STORED_STATE_SLOT_COUNT.is_power_of_two());
const _: () = assert!(generated::STORED_STATE_MAX_PROPERTIES <= 7);
const _: () = assert!(generated::STORED_STATE_NAME_COUNT <= 1 << 11);
const _: () = assert!(generated::STORED_STATE_PROPERTY_PAIR_COUNT <= 1 << 9);
const _: () = assert!(generated::STORED_STATE_PROPERTY_OCCURRENCES <= 1 << 18);

/// Exact persisted-state resolver for the pinned Minecraft 26.2 state universe.
///
/// This is a zero-sized cold-boundary mechanism. It performs no runtime table construction and no
/// allocation. The generated binary is source/runtime-qualified and byte-reproducible in CI.
#[derive(Clone, Copy, Debug, Default)]
pub struct Target262BlockStateResolver;

impl BlockStateResolver for Target262BlockStateResolver {
    type State = BlockStateId;

    fn resolve(&self, name: &str, properties: &[BlockProperty<'_>]) -> Option<Self::State> {
        resolve_target_26_2_block_state(name, properties)
    }
}

/// Resolves one canonicalized saved 26.2 block state to Helve's existing dense identity.
///
/// `properties` must be lexicographically sorted by name, as guaranteed by the stored-section
/// decoder. Unknown, malformed, non-canonical or hash-colliding input fails closed.
#[must_use]
pub fn resolve_target_26_2_block_state(
    name: &str,
    properties: &[BlockProperty<'_>],
) -> Option<BlockStateId> {
    if properties.len() > generated::STORED_STATE_MAX_PROPERTIES {
        return None;
    }

    let fingerprint = canonical_state_fingerprint(name, properties);
    let bucket = usize::try_from(fingerprint & generated::STORED_STATE_BUCKET_MASK).ok()?;
    let displacement_offset =
        scaled_offset(generated::STORED_STATE_DISPLACEMENTS_OFFSET, bucket, 2)?;
    let displacement = read_u16(displacement_offset)?;
    let slot_hash = splitmix64(fingerprint ^ u64::from(displacement));
    let slot = usize::try_from(slot_hash & generated::STORED_STATE_SLOT_MASK).ok()?;
    let slot_offset = scaled_offset(generated::STORED_STATE_SLOTS_OFFSET, slot, 2)?;
    let state = BlockStateId::new(u32::from(read_u16(slot_offset)?))?;

    descriptor_matches(state.as_usize(), name, properties).then_some(state)
}

/// Stable `FNV-1a` fingerprint of the canonical `name[properties]` byte stream.
///
/// The fingerprint narrows lookup to one generated candidate but is never treated as identity.
#[must_use]
pub fn canonical_state_fingerprint(name: &str, properties: &[BlockProperty<'_>]) -> u64 {
    let mut hash = hash_bytes(FNV_OFFSET_BASIS, name.as_bytes());
    if properties.is_empty() {
        return hash;
    }

    hash = hash_byte(hash, b'[');
    for (index, property) in properties.iter().enumerate() {
        if index != 0 {
            hash = hash_byte(hash, b',');
        }
        hash = hash_bytes(hash, property.name.as_bytes());
        hash = hash_byte(hash, b'=');
        hash = hash_bytes(hash, property.value.as_bytes());
    }
    hash_byte(hash, b']')
}

fn descriptor_matches(state_id: usize, name: &str, properties: &[BlockProperty<'_>]) -> bool {
    let Some(descriptor_offset) =
        scaled_offset(generated::STORED_STATE_DESCRIPTORS_OFFSET, state_id, 4)
    else {
        return false;
    };
    let Some(descriptor) = read_u32(descriptor_offset) else {
        return false;
    };
    let Ok(name_id) = usize::try_from(descriptor & STATE_NAME_MASK) else {
        return false;
    };
    let Ok(property_start) = usize::try_from((descriptor >> 11) & STATE_PROPERTY_START_MASK) else {
        return false;
    };
    let Ok(property_len) = usize::try_from(descriptor >> 29) else {
        return false;
    };

    property_len == properties.len()
        && name_id < generated::STORED_STATE_NAME_COUNT
        && name_matches(name_id, name)
        && properties.iter().enumerate().all(|(index, property)| {
            let Some(occurrence) = property_start.checked_add(index) else {
                return false;
            };
            let Some(pair_id) = property_pair_id(occurrence) else {
                return false;
            };
            pair_matches(pair_id, property)
        })
}

fn name_matches(name_id: usize, name: &str) -> bool {
    let Some(index_offset) = scaled_offset(generated::STORED_STATE_NAME_INDEX_OFFSET, name_id, 3)
    else {
        return false;
    };
    let Some(blob_offset) = read_u16(index_offset).map(usize::from) else {
        return false;
    };
    let Some(length_offset) = index_offset.checked_add(2) else {
        return false;
    };
    let Some(len) = read_u8(length_offset).map(usize::from) else {
        return false;
    };
    let Some(start) = generated::STORED_STATE_NAME_BLOB_OFFSET.checked_add(blob_offset) else {
        return false;
    };
    bytes_at(start, len) == Some(name.as_bytes())
}

fn property_pair_id(occurrence: usize) -> Option<usize> {
    if occurrence >= generated::STORED_STATE_PROPERTY_OCCURRENCES {
        return None;
    }
    let bit = occurrence.checked_mul(9)?;
    let absolute = generated::STORED_STATE_PROPERTY_IDS_OFFSET.checked_add(bit / 8)?;
    let word = read_u16(absolute)?;
    Some(usize::from((word >> (bit % 8)) & PROPERTY_ID_MASK))
}

fn pair_matches(pair_id: usize, property: &BlockProperty<'_>) -> bool {
    if pair_id >= generated::STORED_STATE_PROPERTY_PAIR_COUNT {
        return false;
    }
    let Some(index_offset) = scaled_offset(generated::STORED_STATE_PAIR_INDEX_OFFSET, pair_id, 4)
    else {
        return false;
    };
    let Some(blob_offset) = read_u16(index_offset).map(usize::from) else {
        return false;
    };
    let Some(name_len_offset) = index_offset.checked_add(2) else {
        return false;
    };
    let Some(total_len_offset) = index_offset.checked_add(3) else {
        return false;
    };
    let Some(name_len) = read_u8(name_len_offset).map(usize::from) else {
        return false;
    };
    let Some(total_len) = read_u8(total_len_offset).map(usize::from) else {
        return false;
    };
    if total_len <= name_len {
        return false;
    }
    let Some(start) = generated::STORED_STATE_PAIR_BLOB_OFFSET.checked_add(blob_offset) else {
        return false;
    };
    let Some(pair) = bytes_at(start, total_len) else {
        return false;
    };
    pair.get(..name_len) == Some(property.name.as_bytes())
        && pair.get(name_len) == Some(&b'=')
        && pair.get(name_len + 1..) == Some(property.value.as_bytes())
}

fn scaled_offset(base: usize, index: usize, stride: usize) -> Option<usize> {
    base.checked_add(index.checked_mul(stride)?)
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(SPLITMIX_INCREMENT);
    value = (value ^ (value >> 30)).wrapping_mul(SPLITMIX_MIX1);
    value = (value ^ (value >> 27)).wrapping_mul(SPLITMIX_MIX2);
    value ^ (value >> 31)
}

fn hash_byte(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
}

fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash = hash_byte(hash, byte);
    }
    hash
}

fn read_u8(offset: usize) -> Option<u8> {
    generated::STORED_STATE_LOOKUP_DATA.get(offset).copied()
}

fn read_u16(offset: usize) -> Option<u16> {
    let bytes = generated::STORED_STATE_LOOKUP_DATA.get(offset..offset.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(offset: usize) -> Option<u32> {
    let bytes = generated::STORED_STATE_LOOKUP_DATA.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn bytes_at(offset: usize, len: usize) -> Option<&'static [u8]> {
    generated::STORED_STATE_LOOKUP_DATA.get(offset..offset.checked_add(len)?)
}

#[cfg(test)]
mod tests {
    use super::{
        Target262BlockStateResolver, canonical_state_fingerprint, resolve_target_26_2_block_state,
        splitmix64,
    };
    use crate::{
        generated_state_lookup as generated,
        stored_blocks::{BlockProperty, BlockStateResolver},
    };
    use helve_generated::{AIR, BLOCK_STATE_COUNT};

    #[test]
    fn generated_table_shape_matches_target_universe() {
        assert_eq!(
            generated::STORED_STATE_LOOKUP_DATA.len(),
            generated::STORED_STATE_LOOKUP_BYTES
        );
        assert_eq!(generated::STORED_STATE_LOOKUP_COUNT, BLOCK_STATE_COUNT);
    }

    #[test]
    fn stable_hash_witnesses_hold() {
        assert_eq!(
            canonical_state_fingerprint("minecraft:air", &[]),
            0xc480_b16a_4005_8ec2
        );
        assert_eq!(
            canonical_state_fingerprint(
                "minecraft:oak_log",
                &[BlockProperty {
                    name: "axis",
                    value: "y",
                }],
            ),
            0xa628_dde6_f223_4d1f,
        );
        assert_eq!(splitmix64(0), 0xe220_a839_7b1d_cdaf);
    }

    #[test]
    fn generated_target_resolver_accepts_exact_air_and_rejects_unknown() {
        assert_eq!(
            resolve_target_26_2_block_state("minecraft:air", &[]),
            Some(AIR)
        );
        assert_eq!(
            resolve_target_26_2_block_state("minecraft:not_a_real_block", &[]),
            None
        );
    }

    #[test]
    fn resolver_trait_uses_the_same_exact_target_table() {
        let resolver = Target262BlockStateResolver;
        assert_eq!(resolver.resolve("minecraft:air", &[]), Some(AIR));
    }

    #[test]
    fn noncanonical_or_near_match_properties_fail_closed() {
        assert!(
            resolve_target_26_2_block_state(
                "minecraft:oak_log",
                &[BlockProperty {
                    name: "axis",
                    value: "not-a-real-axis",
                }],
            )
            .is_none()
        );
        assert!(
            resolve_target_26_2_block_state(
                "minecraft:oak_log",
                &[
                    BlockProperty {
                        name: "waterlogged",
                        value: "false",
                    },
                    BlockProperty {
                        name: "axis",
                        value: "y",
                    },
                ],
            )
            .is_none()
        );
    }
}
