use core::mem::size_of;

const BLOCKS_PER_SECTION_AXIS: i32 = 16;

/// Fail-closed construction errors for a contiguous vertical section lattice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerticalSectionLatticeError {
    /// A live lattice must contain at least one logical section.
    Empty,
    /// The number of sections cannot be represented by the compact lattice identity.
    SectionCountTooLarge { count: usize },
    /// The requested block-space range cannot be represented by `i32` world coordinates.
    BlockRangeOverflow,
}

/// Compact validated vertical layout shared by chunk access paths.
///
/// Construction resolves all signed section/block boundary arithmetic once. After validation,
/// ordinary block-Y lookup is a pair of range comparisons, one subtraction and bit operations; it
/// does not perform signed Euclidean division in the HOT path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerticalSectionLattice {
    min_block_y: i32,
    max_block_y_exclusive: i32,
    min_section_y: i32,
    section_count: u16,
}

impl VerticalSectionLattice {
    /// Creates one exact contiguous section lattice.
    ///
    /// # Errors
    ///
    /// Rejects empty lattices, section counts that do not fit the compact representation, and
    /// block-space ranges that overflow `i32` coordinates.
    pub fn new(
        min_section_y: i32,
        section_count: usize,
    ) -> Result<Self, VerticalSectionLatticeError> {
        if section_count == 0 {
            return Err(VerticalSectionLatticeError::Empty);
        }
        let section_count = u16::try_from(section_count).map_err(|_| {
            VerticalSectionLatticeError::SectionCountTooLarge {
                count: section_count,
            }
        })?;
        let min_block_y = min_section_y
            .checked_mul(BLOCKS_PER_SECTION_AXIS)
            .ok_or(VerticalSectionLatticeError::BlockRangeOverflow)?;
        let block_height = i32::from(section_count)
            .checked_mul(BLOCKS_PER_SECTION_AXIS)
            .ok_or(VerticalSectionLatticeError::BlockRangeOverflow)?;
        let max_block_y_exclusive = min_block_y
            .checked_add(block_height)
            .ok_or(VerticalSectionLatticeError::BlockRangeOverflow)?;

        Ok(Self {
            min_block_y,
            max_block_y_exclusive,
            min_section_y,
            section_count,
        })
    }

    /// Lowest world block Y covered by this lattice.
    #[must_use]
    pub const fn min_block_y(self) -> i32 {
        self.min_block_y
    }

    /// Exclusive upper world block Y covered by this lattice.
    #[must_use]
    pub const fn max_block_y_exclusive(self) -> i32 {
        self.max_block_y_exclusive
    }

    /// Lowest logical section Y covered by this lattice.
    #[must_use]
    pub const fn min_section_y(self) -> i32 {
        self.min_section_y
    }

    /// Number of contiguous logical section slots.
    #[must_use]
    pub const fn section_count(self) -> usize {
        self.section_count as usize
    }

    /// Resolves one admitted world block Y into its zero-based section slot and section-local Y.
    ///
    /// The constructor has already proved the range subtraction cannot overflow for an in-range
    /// coordinate. Division/remainder by 16 therefore become one shift and one mask over the
    /// validated coordinate. Callers needing both values should use this method so the range check
    /// and subtraction are performed once.
    #[inline]
    #[must_use]
    pub fn resolve_block_y(self, block_y: i32) -> Option<(usize, u8)> {
        if block_y < self.min_block_y || block_y >= self.max_block_y_exclusive {
            return None;
        }
        let relative = block_y - self.min_block_y;
        let index = usize::try_from(relative >> 4).ok()?;
        let local_y = u8::try_from(block_y & 15).ok()?;
        Some((index, local_y))
    }

    /// Returns the zero-based section slot containing `block_y`.
    #[inline]
    #[must_use]
    pub fn section_index_for_block_y(self, block_y: i32) -> Option<usize> {
        self.resolve_block_y(block_y).map(|(index, _)| index)
    }

    /// Returns the local Y coordinate within the containing section for an admitted block Y.
    #[inline]
    #[must_use]
    pub fn local_y_for_block_y(self, block_y: i32) -> Option<u8> {
        self.resolve_block_y(block_y).map(|(_, local_y)| local_y)
    }

    /// Resolves one logical section Y directly into its zero-based contiguous slot.
    ///
    /// This is the section-space counterpart of [`Self::resolve_block_y`]. It avoids converting a
    /// section identity back into block coordinates and therefore performs only a checked range
    /// subtraction plus one integer conversion on the admitted path.
    #[inline]
    #[must_use]
    pub fn section_index_for_section_y(self, section_y: i32) -> Option<usize> {
        let relative = section_y.checked_sub(self.min_section_y)?;
        if relative < 0 || relative >= i32::from(self.section_count) {
            return None;
        }
        usize::try_from(relative).ok()
    }

    /// Returns the logical section Y represented by one zero-based slot.
    #[must_use]
    pub fn section_y_for_index(self, index: usize) -> Option<i32> {
        if index >= self.section_count() {
            return None;
        }
        let offset = i32::try_from(index).ok()?;
        self.min_section_y.checked_add(offset)
    }
}

const _: () = assert!(size_of::<VerticalSectionLattice>() == 16);

#[cfg(test)]
mod tests {
    use super::{VerticalSectionLattice, VerticalSectionLatticeError};

    #[test]
    fn standard_dimension_lattices_have_exact_bounds() {
        let overworld = VerticalSectionLattice::new(-4, 24).expect("Overworld lattice");
        assert_eq!(overworld.min_block_y(), -64);
        assert_eq!(overworld.max_block_y_exclusive(), 320);
        assert_eq!(overworld.min_section_y(), -4);
        assert_eq!(overworld.section_count(), 24);

        let nether = VerticalSectionLattice::new(0, 16).expect("Nether lattice");
        assert_eq!(nether.min_block_y(), 0);
        assert_eq!(nether.max_block_y_exclusive(), 256);
        assert_eq!(nether.section_count(), 16);
    }

    #[test]
    fn empty_oversized_and_overflowing_lattices_fail_closed() {
        assert_eq!(
            VerticalSectionLattice::new(0, 0),
            Err(VerticalSectionLatticeError::Empty)
        );
        assert_eq!(
            VerticalSectionLattice::new(0, usize::from(u16::MAX) + 1),
            Err(VerticalSectionLatticeError::SectionCountTooLarge {
                count: usize::from(u16::MAX) + 1,
            })
        );
        assert_eq!(
            VerticalSectionLattice::new(i32::MAX, 1),
            Err(VerticalSectionLatticeError::BlockRangeOverflow)
        );
    }

    #[test]
    fn resolved_indices_match_euclidean_reference_across_negative_coordinates() {
        let lattice = VerticalSectionLattice::new(-8, 40).expect("test lattice");
        for y in -160_i32..560 {
            let expected = if (-128_i32..512).contains(&y) {
                Some((
                    usize::try_from(y.div_euclid(16) + 8).expect("non-negative slot"),
                    u8::try_from(y.rem_euclid(16)).expect("local y"),
                ))
            } else {
                None
            };
            assert_eq!(lattice.resolve_block_y(y), expected, "y={y}");
            assert_eq!(
                lattice.section_index_for_block_y(y),
                expected.map(|(index, _)| index),
                "y={y}"
            );
            assert_eq!(
                lattice.local_y_for_block_y(y),
                expected.map(|(_, local_y)| local_y),
                "y={y}"
            );
        }
    }

    #[test]
    fn exact_boundary_and_extreme_negative_ranges_are_safe() {
        let min_section_y = i32::MIN / 16;
        let lattice = VerticalSectionLattice::new(min_section_y, 2).expect("minimum lattice");
        assert_eq!(lattice.min_block_y(), i32::MIN);
        assert_eq!(lattice.resolve_block_y(i32::MIN), Some((0, 0)));
        assert_eq!(lattice.resolve_block_y(i32::MIN + 15), Some((0, 15)));
        assert_eq!(lattice.resolve_block_y(i32::MIN + 16), Some((1, 0)));
        assert_eq!(lattice.resolve_block_y(i32::MIN + 31), Some((1, 15)));
        assert_eq!(lattice.resolve_block_y(i32::MIN + 32), None);
    }

    #[test]
    fn section_index_round_trip_is_exact() {
        let lattice = VerticalSectionLattice::new(-64, 257).expect("wide lattice");
        for index in 0..lattice.section_count() {
            let section_y = lattice.section_y_for_index(index).expect("valid index");
            assert_eq!(lattice.section_index_for_section_y(section_y), Some(index));
            let block_y = section_y.checked_mul(16).expect("test block y");
            assert_eq!(lattice.resolve_block_y(block_y), Some((index, 0)));
        }
        assert_eq!(lattice.section_y_for_index(lattice.section_count()), None);
        assert_eq!(lattice.section_index_for_section_y(-65), None);
        assert_eq!(lattice.section_index_for_section_y(193), None);
    }
}
