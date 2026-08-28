//! Allocation-free lookup core for persisted block-state identities.
//!
//! The generated cold table stores canonical target state keys behind a stable fingerprint index.
//! Lookup hashes the already-canonicalized `(Name, Properties)` view without constructing a string,
//! binary-searches the fingerprint array, then verifies exact canonical key bytes before returning a
//! dense semantic state ID. Fingerprints are therefore an index only, never identity.

use crate::stored_blocks::BlockProperty;

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Compact metadata paired positionally with one generated fingerprint.
///
/// The frozen 26.2 target proves its dense state universe fits in `u16`. Keeping fingerprints in a
/// separate SoA array lets binary search touch only eight bytes per probe; this row is exactly eight
/// bytes and is read only after a matching fingerprint has been found.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredStateLookupRow {
    key_offset: u32,
    key_len: u16,
    state_id: u16,
}

const _: () = assert!(core::mem::size_of::<StoredStateLookupRow>() == 8);

impl StoredStateLookupRow {
    /// Creates one generated exact-match row.
    #[must_use]
    pub const fn new(key_offset: u32, key_len: u16, state_id: u16) -> Self {
        Self {
            key_offset,
            key_len,
            state_id,
        }
    }

    /// Byte offset of the canonical key in the generated blob.
    #[must_use]
    pub const fn key_offset(self) -> u32 {
        self.key_offset
    }

    /// UTF-8 byte length of the canonical key.
    #[must_use]
    pub const fn key_len(self) -> u16 {
        self.key_len
    }

    /// Dense target semantic state ID associated with the exact key.
    #[must_use]
    pub const fn state_id(self) -> u16 {
        self.state_id
    }
}

/// Borrowed generated exact-lookup table.
#[derive(Clone, Copy, Debug)]
pub struct StoredStateLookup<'a> {
    key_blob: &'a str,
    fingerprints: &'a [u64],
    rows: &'a [StoredStateLookupRow],
}

impl<'a> StoredStateLookup<'a> {
    /// Binds the generated canonical-key blob and positionally paired SoA index.
    #[must_use]
    pub const fn new(
        key_blob: &'a str,
        fingerprints: &'a [u64],
        rows: &'a [StoredStateLookupRow],
    ) -> Self {
        Self {
            key_blob,
            fingerprints,
            rows,
        }
    }

    /// Resolves one saved state to the dense raw semantic ID.
    ///
    /// No allocation or temporary canonical string is required. A hash collision only widens the
    /// exact comparison range; it can never produce a false semantic match.
    #[must_use]
    pub fn resolve_raw(&self, name: &str, properties: &[BlockProperty<'_>]) -> Option<u32> {
        let fingerprint = canonical_state_fingerprint(name, properties);
        self.resolve_with_fingerprint(fingerprint, name, properties)
    }

    fn resolve_with_fingerprint(
        &self,
        fingerprint: u64,
        name: &str,
        properties: &[BlockProperty<'_>],
    ) -> Option<u32> {
        if self.fingerprints.len() != self.rows.len() {
            return None;
        }

        let mut left = 0_usize;
        let mut right = self.fingerprints.len();
        while left < right {
            let middle = left + (right - left) / 2;
            if self.fingerprints[middle] < fingerprint {
                left = middle + 1;
            } else {
                right = middle;
            }
        }

        let mut index = left;
        while self.fingerprints.get(index).copied() == Some(fingerprint) {
            let row = *self.rows.get(index)?;
            if let Some(expected) = self.row_key(row)
                && canonical_key_matches(expected, name, properties)
            {
                return Some(u32::from(row.state_id));
            }
            index += 1;
        }
        None
    }

    fn row_key(&self, row: StoredStateLookupRow) -> Option<&str> {
        let start = usize::try_from(row.key_offset).ok()?;
        let end = start.checked_add(usize::from(row.key_len))?;
        self.key_blob.get(start..end)
    }
}

/// Stable FNV-1a fingerprint of the canonical state key represented by `name + properties`.
///
/// Properties are expected in the lexicographic order guaranteed by the persisted-state decoder.
/// The byte stream is exactly `name` for property-free states and
/// `name[prop=value,...]` otherwise.
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

fn hash_byte(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
}

fn hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash = hash_byte(hash, byte);
    }
    hash
}

fn canonical_key_matches(expected: &str, name: &str, properties: &[BlockProperty<'_>]) -> bool {
    let Some(expected_len) = canonical_key_len(name, properties) else {
        return false;
    };
    if expected.len() != expected_len {
        return false;
    }

    let bytes = expected.as_bytes();
    let mut cursor = 0_usize;
    if !consume(bytes, &mut cursor, name.as_bytes()) {
        return false;
    }
    if properties.is_empty() {
        return cursor == bytes.len();
    }
    if !consume(bytes, &mut cursor, b"[") {
        return false;
    }
    for (index, property) in properties.iter().enumerate() {
        if index != 0 && !consume(bytes, &mut cursor, b",") {
            return false;
        }
        if !consume(bytes, &mut cursor, property.name.as_bytes())
            || !consume(bytes, &mut cursor, b"=")
            || !consume(bytes, &mut cursor, property.value.as_bytes())
        {
            return false;
        }
    }
    consume(bytes, &mut cursor, b"]") && cursor == bytes.len()
}

fn canonical_key_len(name: &str, properties: &[BlockProperty<'_>]) -> Option<usize> {
    let mut len = name.len();
    if properties.is_empty() {
        return Some(len);
    }
    len = len.checked_add(2)?;
    for (index, property) in properties.iter().enumerate() {
        if index != 0 {
            len = len.checked_add(1)?;
        }
        len = len.checked_add(property.name.len())?;
        len = len.checked_add(1)?;
        len = len.checked_add(property.value.len())?;
    }
    Some(len)
}

fn consume(bytes: &[u8], cursor: &mut usize, fragment: &[u8]) -> bool {
    let Some(end) = (*cursor).checked_add(fragment.len()) else {
        return false;
    };
    if bytes.get(*cursor..end) != Some(fragment) {
        return false;
    }
    *cursor = end;
    true
}

#[cfg(test)]
mod tests {
    use super::{
        FNV_OFFSET_BASIS, StoredStateLookup, StoredStateLookupRow, canonical_state_fingerprint,
        hash_bytes,
    };
    use crate::stored_blocks::BlockProperty;

    #[test]
    fn row_layout_is_exactly_eight_bytes() {
        assert_eq!(core::mem::size_of::<StoredStateLookupRow>(), 8);
    }

    #[test]
    fn structured_fingerprint_matches_exact_canonical_bytes() {
        let properties = [
            BlockProperty {
                name: "axis",
                value: "y",
            },
            BlockProperty {
                name: "waterlogged",
                value: "false",
            },
        ];
        let structured = canonical_state_fingerprint("minecraft:oak_log", &properties);
        let canonical = hash_bytes(
            FNV_OFFSET_BASIS,
            b"minecraft:oak_log[axis=y,waterlogged=false]",
        );
        assert_eq!(structured, canonical);
    }

    #[test]
    fn exact_lookup_resolves_property_free_and_property_states() {
        let air = "minecraft:air";
        let log = "minecraft:oak_log[axis=y]";
        let blob = "minecraft:airminecraft:oak_log[axis=y]";
        let mut pairs = [
            (
                canonical_state_fingerprint("minecraft:air", &[]),
                StoredStateLookupRow::new(
                    0,
                    u16::try_from(air.len()).expect("test key length"),
                    0,
                ),
            ),
            (
                canonical_state_fingerprint(
                    "minecraft:oak_log",
                    &[BlockProperty {
                        name: "axis",
                        value: "y",
                    }],
                ),
                StoredStateLookupRow::new(
                    u32::try_from(air.len()).expect("test offset"),
                    u16::try_from(log.len()).expect("test key length"),
                    7,
                ),
            ),
        ];
        pairs.sort_unstable_by_key(|pair| pair.0);
        let fingerprints = pairs.map(|pair| pair.0);
        let rows = pairs.map(|pair| pair.1);
        let lookup = StoredStateLookup::new(blob, &fingerprints, &rows);

        assert_eq!(lookup.resolve_raw("minecraft:air", &[]), Some(0));
        assert_eq!(
            lookup.resolve_raw(
                "minecraft:oak_log",
                &[BlockProperty {
                    name: "axis",
                    value: "y",
                }],
            ),
            Some(7)
        );
        assert_eq!(
            lookup.resolve_raw(
                "minecraft:oak_log",
                &[BlockProperty {
                    name: "axis",
                    value: "x",
                }],
            ),
            None
        );
    }

    #[test]
    fn fingerprint_collision_still_requires_exact_key() {
        let blob = "minecraft:airminecraft:stone";
        let fingerprints = [7, 7];
        let rows = [
            StoredStateLookupRow::new(0, 13, 1),
            StoredStateLookupRow::new(13, 15, 2),
        ];
        let lookup = StoredStateLookup::new(blob, &fingerprints, &rows);

        assert_eq!(
            lookup.resolve_with_fingerprint(7, "minecraft:stone", &[]),
            Some(2)
        );
        assert_eq!(
            lookup.resolve_with_fingerprint(7, "minecraft:dirt", &[]),
            None
        );
    }

    #[test]
    fn malformed_generated_ranges_fail_closed() {
        let fingerprints = [1];
        let rows = [StoredStateLookupRow::new(u32::MAX, 10, 9)];
        let lookup = StoredStateLookup::new("minecraft:air", &fingerprints, &rows);
        assert_eq!(
            lookup.resolve_with_fingerprint(1, "minecraft:air", &[]),
            None
        );

        let lookup = StoredStateLookup::new("minecraft:air", &[1, 1], &rows);
        assert_eq!(
            lookup.resolve_with_fingerprint(1, "minecraft:air", &[]),
            None
        );
    }
}
