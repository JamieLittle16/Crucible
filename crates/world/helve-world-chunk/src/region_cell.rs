use core::marker::PhantomData;

use helve_types::ChunkPos;

/// Coarse regionizer cell coordinate in chunk-space.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RegionCellCoord {
    pub x: i32,
    pub z: i32,
}

/// Exact address of one chunk inside one coarse regionizer cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegionCellAddress {
    cell: RegionCellCoord,
    local_x: u16,
    local_z: u16,
    slot: u32,
}

impl RegionCellAddress {
    /// Coarse region-cell coordinate containing the chunk.
    #[must_use]
    pub const fn cell(self) -> RegionCellCoord {
        self.cell
    }

    /// Zero-based X coordinate inside the cell.
    #[must_use]
    pub const fn local_x(self) -> u16 {
        self.local_x
    }

    /// Zero-based Z coordinate inside the cell.
    #[must_use]
    pub const fn local_z(self) -> u16 {
        self.local_z
    }

    /// Dense row-major slot inside the cell.
    #[must_use]
    pub const fn slot(self) -> u32 {
        self.slot
    }
}

/// Compile-time power-of-two region-cell layout.
///
/// `SHIFT` selects `1 << SHIFT` chunks on each horizontal axis. The type is zero-sized: production
/// composition can select a region-cell granularity statically, so ordinary address resolution pays
/// no runtime configuration lookup or dynamic dispatch.
///
/// The current helper admits shifts up to 15, which keeps local coordinates in `u16` and dense slot
/// identities in `u32`. Real production candidates are expected to be far smaller; this broad range
/// exists so qualification can compare mechanisms without changing the semantic API.
#[derive(Clone, Copy, Debug, Default)]
pub struct RegionCellLayout<const SHIFT: u32>(PhantomData<()>);

impl<const SHIFT: u32> RegionCellLayout<SHIFT> {
    const CHECK: () = assert!(SHIFT <= 15, "region cell shift must be <= 15");

    /// Number of chunks on each horizontal axis.
    #[must_use]
    pub const fn side_chunks() -> u32 {
        let () = Self::CHECK;
        1_u32 << SHIFT
    }

    /// Number of exact chunk slots in one cell.
    #[must_use]
    pub const fn slot_count() -> u32 {
        let side = Self::side_chunks();
        side * side
    }

    /// Resolves an exact chunk position into a coarse cell plus dense local slot.
    ///
    /// # Panics
    ///
    /// Panics when instantiated with `SHIFT > 15`. For every admitted layout, the mask and masked
    /// local coordinates are mathematically bounded to `i32`/`u16`; the checked conversions below
    /// therefore cannot otherwise fail.
    #[inline]
    #[must_use]
    pub fn address(chunk: ChunkPos) -> RegionCellAddress {
        let () = Self::CHECK;
        let mask = i32::try_from(Self::side_chunks() - 1).expect("validated region-cell mask");
        let local_x = u16::try_from(chunk.x & mask).expect("masked local x fits u16");
        let local_z = u16::try_from(chunk.z & mask).expect("masked local z fits u16");
        let side = Self::side_chunks();
        let slot = u32::from(local_z) * side + u32::from(local_x);
        RegionCellAddress {
            cell: RegionCellCoord {
                x: chunk.x >> SHIFT,
                z: chunk.z >> SHIFT,
            },
            local_x,
            local_z,
            slot,
        }
    }

    /// Reconstructs one chunk position from a cell and exact dense slot.
    ///
    /// # Errors
    ///
    /// Returns `None` for an out-of-range slot or a coordinate combination that would exceed the
    /// semantic `i32` chunk-coordinate range.
    #[must_use]
    pub fn chunk_for_slot(cell: RegionCellCoord, slot: u32) -> Option<ChunkPos> {
        let side = Self::side_chunks();
        if slot >= Self::slot_count() {
            return None;
        }
        let local_x = slot % side;
        let local_z = slot / side;
        let side_i64 = i64::from(side);
        let chunk_x = i64::from(cell.x)
            .checked_mul(side_i64)?
            .checked_add(i64::from(local_x))?;
        let chunk_z = i64::from(cell.z)
            .checked_mul(side_i64)?
            .checked_add(i64::from(local_z))?;
        Some(ChunkPos {
            x: i32::try_from(chunk_x).ok()?,
            z: i32::try_from(chunk_z).ok()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use core::mem::size_of;

    use helve_types::ChunkPos;

    use super::{RegionCellCoord, RegionCellLayout};

    fn assert_matches_reference<const SHIFT: u32>(chunk: ChunkPos) {
        let side = i32::try_from(RegionCellLayout::<SHIFT>::side_chunks()).expect("test side");
        let address = RegionCellLayout::<SHIFT>::address(chunk);
        assert_eq!(address.cell().x, chunk.x.div_euclid(side));
        assert_eq!(address.cell().z, chunk.z.div_euclid(side));
        assert_eq!(
            address.local_x(),
            u16::try_from(chunk.x.rem_euclid(side)).expect("reference local x")
        );
        assert_eq!(
            address.local_z(),
            u16::try_from(chunk.z.rem_euclid(side)).expect("reference local z")
        );
        let expected_slot = u32::from(address.local_z()) * RegionCellLayout::<SHIFT>::side_chunks()
            + u32::from(address.local_x());
        assert_eq!(address.slot(), expected_slot);
        assert_eq!(
            RegionCellLayout::<SHIFT>::chunk_for_slot(address.cell(), address.slot()),
            Some(chunk)
        );
    }

    #[test]
    fn layouts_are_zero_sized_static_policy_types() {
        assert_eq!(size_of::<RegionCellLayout<0>>(), 0);
        assert_eq!(size_of::<RegionCellLayout<3>>(), 0);
        assert_eq!(size_of::<RegionCellLayout<8>>(), 0);
    }

    #[test]
    fn common_candidate_sizes_have_exact_capacity() {
        assert_eq!(RegionCellLayout::<0>::side_chunks(), 1);
        assert_eq!(RegionCellLayout::<0>::slot_count(), 1);
        assert_eq!(RegionCellLayout::<2>::side_chunks(), 4);
        assert_eq!(RegionCellLayout::<2>::slot_count(), 16);
        assert_eq!(RegionCellLayout::<3>::side_chunks(), 8);
        assert_eq!(RegionCellLayout::<3>::slot_count(), 64);
        assert_eq!(RegionCellLayout::<4>::side_chunks(), 16);
        assert_eq!(RegionCellLayout::<4>::slot_count(), 256);
    }

    #[test]
    fn negative_and_positive_chunk_coordinates_match_euclidean_reference() {
        for z in -257..=257 {
            for x in -257..=257 {
                let chunk = ChunkPos { x, z };
                assert_matches_reference::<0>(chunk);
                assert_matches_reference::<1>(chunk);
                assert_matches_reference::<2>(chunk);
                assert_matches_reference::<3>(chunk);
                assert_matches_reference::<4>(chunk);
                assert_matches_reference::<8>(chunk);
            }
        }
    }

    #[test]
    fn integer_extremes_round_trip_for_multiple_layouts() {
        let values = [
            i32::MIN,
            i32::MIN + 1,
            -32_769,
            -32_768,
            -1,
            0,
            1,
            32_767,
            32_768,
            i32::MAX - 1,
            i32::MAX,
        ];
        for &z in &values {
            for &x in &values {
                let chunk = ChunkPos { x, z };
                assert_matches_reference::<1>(chunk);
                assert_matches_reference::<3>(chunk);
                assert_matches_reference::<8>(chunk);
                assert_matches_reference::<15>(chunk);
            }
        }
    }

    #[test]
    fn arbitrary_cell_slot_reconstruction_fails_closed_on_overflow() {
        assert_eq!(
            RegionCellLayout::<3>::chunk_for_slot(RegionCellCoord { x: 0, z: 0 }, 64),
            None
        );
        assert_eq!(
            RegionCellLayout::<15>::chunk_for_slot(RegionCellCoord { x: i32::MAX, z: 0 }, 0,),
            None
        );
    }
}
