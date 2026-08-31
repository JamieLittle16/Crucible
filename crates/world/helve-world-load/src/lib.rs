//! Cold composition from a fully decoded stored chunk into live Helve residency.
//!
//! Persisted-format parsing belongs to `helve-world-import`; chunk identity/lifecycle belongs to
//! `helve-world-runtime`. This crate is the narrow composition seam between them. It does not own a
//! second world representation, target packet state, scheduler placement, persistence policy, or
//! network publication.
//!
//! Installation is intentionally structural rather than cell-wise: block-bearing imported sections
//! are moved directly into the resident column, omitted logical sections are synthesized by a
//! caller-selected empty-section factory, and the final contiguous section vector is handed directly
//! to the runtime. No 4096-cell semantic copy is introduced by this layer.

#![forbid(unsafe_code)]

use helve_world_contract::BlockSection;
use helve_world_import::{ImportedStoredChunk, StoredChunkSourceMetadata};
use helve_world_runtime::{DimensionInstance, LoadChunkError, ResidentChunkHandle};

const BLOCKS_PER_SECTION_AXIS: i32 = 16;

/// Successful cold-to-resident composition result.
///
/// Persisted source metadata is returned to the caller for evidence/diagnostics. It is deliberately
/// not attached to authoritative live chunk state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InstalledStoredChunk {
    /// Exact live chunk incarnation created by this installation.
    pub handle: ResidentChunkHandle,
    /// Non-authoritative metadata from the stored source transaction.
    pub source: StoredChunkSourceMetadata,
}

/// Fail-closed errors while composing sparse imported sections into one resident chunk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidentInstallError {
    /// A persisted block-bearing section lies outside the loaded dimension's frozen lattice.
    SectionOutsideDimension {
        /// Rejected logical section Y.
        section_y: i32,
        /// Lowest admitted logical section Y.
        min_section_y: i32,
        /// Exclusive upper logical section Y.
        max_section_y_exclusive: i32,
    },
    /// Defensive rejection of duplicate section Y values at the composition seam.
    ///
    /// The exact-target importer already rejects these; retaining the check keeps this public seam
    /// fail-closed if another qualified imported-chunk producer is added later.
    DuplicateSectionY {
        /// Repeated logical section Y.
        section_y: i32,
    },
    /// Live lifecycle admission rejected the final chunk column.
    Load(LoadChunkError),
}

impl From<LoadChunkError> for ResidentInstallError {
    fn from(value: LoadChunkError) -> Self {
        Self::Load(value)
    }
}

/// Atomically installs one fully decoded stored block chunk into a loaded dimension.
///
/// The persisted sparse section vector is sorted in place, then its section objects are moved into
/// exactly one final contiguous vector. Missing logical slots are created by `make_empty_section`.
/// On success that vector becomes the resident chunk's section backing; this seam never constructs a
/// second per-cell image.
///
/// Range/duplicate validation completes before the empty factory runs. Runtime duplicate-residency
/// admission happens only after the final column is composed so the ordinary successful path pays
/// the runtime's single authoritative sparse-directory probe rather than a redundant preflight
/// lookup. Any runtime load failure leaves the dimension unchanged and drops the uncommitted column.
///
/// # Errors
///
/// Returns [`ResidentInstallError`] when the position is already resident, an imported section lies
/// outside the dimension lattice, a duplicate section Y reaches this seam, generation/lattice
/// construction fails, or the runtime otherwise rejects residency.
pub fn install_imported_chunk<S, Section, MakeEmpty>(
    dimension: &mut DimensionInstance<S, Section>,
    imported: ImportedStoredChunk<Section>,
    mut make_empty_section: MakeEmpty,
) -> Result<InstalledStoredChunk, ResidentInstallError>
where
    S: Copy + Eq,
    Section: BlockSection<S>,
    MakeEmpty: FnMut() -> Section,
{
    let position = imported.blocks.header.position;
    let profile = dimension.profile();
    let min_section_y = profile.min_section_y();
    let max_section_y_exclusive = profile
        .max_block_y_exclusive()
        .div_euclid(BLOCKS_PER_SECTION_AXIS);
    let section_count = profile.section_count();

    let source = imported.source;
    let mut sparse_sections = imported.blocks.sections;
    sparse_sections.sort_unstable_by_key(|section| section.section_y);

    let mut previous_y = None;
    for section in &sparse_sections {
        let section_y = i32::from(section.section_y);
        if section_y < min_section_y || section_y >= max_section_y_exclusive {
            return Err(ResidentInstallError::SectionOutsideDimension {
                section_y,
                min_section_y,
                max_section_y_exclusive,
            });
        }
        if previous_y == Some(section_y) {
            return Err(ResidentInstallError::DuplicateSectionY { section_y });
        }
        previous_y = Some(section_y);
    }

    let mut sparse_sections = sparse_sections.into_iter().peekable();
    let mut resident_sections = Vec::with_capacity(section_count);
    let mut logical_y = min_section_y;
    for _ in 0..section_count {
        let imported_here = sparse_sections
            .peek()
            .is_some_and(|section| i32::from(section.section_y) == logical_y);
        if imported_here {
            if let Some(section) = sparse_sections.next() {
                resident_sections.push(section.section);
            }
        } else {
            resident_sections.push(make_empty_section());
        }
        logical_y += 1;
    }
    debug_assert_eq!(logical_y, max_section_y_exclusive);
    debug_assert!(sparse_sections.next().is_none());
    debug_assert_eq!(resident_sections.len(), section_count);
    debug_assert_eq!(resident_sections.capacity(), section_count);

    let handle = dimension.load_chunk(position, resident_sections)?;
    Ok(InstalledStoredChunk { handle, source })
}

#[cfg(test)]
mod tests {
    use helve_types::{BlockPos, ChunkGeneration, ChunkPos, DimensionId, DimensionTypeId};
    use helve_world_contract::{BlockStateFacts, SectionStateFacts};
    use helve_world_import::{
        ChunkCompression, ImportedBlockSection, ImportedChunkBlocks, ImportedStoredChunk,
        StoredChunkHeader, StoredChunkSourceMetadata, TARGET_DATA_VERSION_26_2,
    };
    use helve_world_reference::DirectBlockSection;
    use helve_world_runtime::{
        DimensionInstance, DimensionRuntimeProfile, LoadChunkError, ResidentChunkAccessError,
    };

    use super::{ResidentInstallError, install_imported_chunk};

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum State {
        Air,
        Stone,
        Dirt,
    }

    struct Facts;

    impl BlockStateFacts<State> for Facts {
        fn facts(&self, state: State) -> SectionStateFacts {
            SectionStateFacts::new(state != State::Air, false, false, false)
        }
    }

    fn profile() -> DimensionRuntimeProfile {
        DimensionRuntimeProfile::new(DimensionTypeId(2), -16, 48, true)
            .expect("three-section test profile")
    }

    fn section(state: State) -> DirectBlockSection<State> {
        DirectBlockSection::filled(state, &Facts)
    }

    fn imported(
        position: ChunkPos,
        sections: &[(i8, State)],
    ) -> ImportedStoredChunk<DirectBlockSection<State>> {
        ImportedStoredChunk {
            blocks: ImportedChunkBlocks {
                header: StoredChunkHeader {
                    data_version: TARGET_DATA_VERSION_26_2,
                    position,
                    stored_section_count: sections.len(),
                },
                sections: sections
                    .iter()
                    .map(|(section_y, state)| ImportedBlockSection {
                        section_y: *section_y,
                        section: section(*state),
                    })
                    .collect(),
            },
            source: StoredChunkSourceMetadata {
                region_timestamp: 17,
                compression: ChunkCompression::Zlib,
                external: false,
            },
        }
    }

    fn origin(position: ChunkPos, y: i32) -> BlockPos {
        BlockPos {
            x: position.x * 16,
            y,
            z: position.z * 16,
        }
    }

    #[test]
    fn sparse_stored_sections_become_one_exact_contiguous_resident_lattice() {
        let profile = profile();
        let mut dimension = DimensionInstance::new(DimensionId(4), profile);
        let position = ChunkPos { x: -2, z: 5 };
        let mut empty_calls = 0;

        let installed = install_imported_chunk(
            &mut dimension,
            imported(position, &[(1, State::Dirt), (-1, State::Stone)]),
            || {
                empty_calls += 1;
                section(State::Air)
            },
        )
        .expect("valid sparse stored chunk installs");

        assert_eq!(empty_calls, 1);
        assert_eq!(dimension.resident_chunk_count(), 1);
        assert_eq!(installed.source.region_timestamp, 17);
        let chunk = dimension
            .resolve_chunk(installed.handle)
            .expect("new resident handle resolves");
        assert_eq!(chunk.min_section_y(), -1);
        assert_eq!(chunk.section_count(), 3);
        assert_eq!(chunk.get_block(origin(position, -16)), Ok(State::Stone));
        assert_eq!(chunk.get_block(origin(position, 0)), Ok(State::Air));
        assert_eq!(chunk.get_block(origin(position, 16)), Ok(State::Dirt));
        assert_eq!(chunk.masks().non_air_bits(), 0b101);
        assert!(chunk.masks_match_recomputation());
    }

    #[test]
    fn out_of_range_and_duplicate_sections_fail_before_empty_construction() {
        let profile = profile();
        let position = ChunkPos { x: 0, z: 0 };

        let mut dimension = DimensionInstance::new(DimensionId(1), profile);
        let mut out_of_range_empty_calls = 0;
        let error = install_imported_chunk(
            &mut dimension,
            imported(position, &[(2, State::Stone)]),
            || {
                out_of_range_empty_calls += 1;
                section(State::Air)
            },
        )
        .expect_err("section above the lattice must fail");
        assert_eq!(
            error,
            ResidentInstallError::SectionOutsideDimension {
                section_y: 2,
                min_section_y: -1,
                max_section_y_exclusive: 2,
            }
        );
        assert_eq!(out_of_range_empty_calls, 0);
        assert_eq!(dimension.resident_chunk_count(), 0);

        let mut duplicate_empty_calls = 0;
        let error = install_imported_chunk(
            &mut dimension,
            imported(position, &[(0, State::Stone), (0, State::Dirt)]),
            || {
                duplicate_empty_calls += 1;
                section(State::Air)
            },
        )
        .expect_err("duplicate logical section must fail");
        assert_eq!(
            error,
            ResidentInstallError::DuplicateSectionY { section_y: 0 }
        );
        assert_eq!(duplicate_empty_calls, 0);
        assert_eq!(dimension.resident_chunk_count(), 0);
    }

    #[test]
    fn already_resident_position_fails_atomically_after_cold_column_composition() {
        let profile = profile();
        let mut dimension = DimensionInstance::new(DimensionId(7), profile);
        let position = ChunkPos { x: 3, z: -4 };
        let first = dimension
            .load_chunk(
                position,
                vec![section(State::Air), section(State::Air), section(State::Air)],
            )
            .expect("baseline resident chunk");
        let mut empty_calls = 0;

        let error = install_imported_chunk(
            &mut dimension,
            imported(position, &[(0, State::Stone)]),
            || {
                empty_calls += 1;
                section(State::Air)
            },
        )
        .expect_err("duplicate residency must fail");
        assert_eq!(
            error,
            ResidentInstallError::Load(LoadChunkError::AlreadyResident { handle: first })
        );
        assert_eq!(empty_calls, 2);
        assert_eq!(dimension.resident_chunk_count(), 1);
        assert_eq!(dimension.discover_chunk(position), Some(first));
        let resident = dimension
            .resolve_chunk(first)
            .expect("existing chunk remains authoritative");
        assert_eq!(resident.get_block(origin(position, 0)), Ok(State::Air));
    }

    #[test]
    fn unload_reload_recreates_semantics_with_new_generation() {
        let profile = profile();
        let mut dimension = DimensionInstance::new(DimensionId(9), profile);
        let position = ChunkPos { x: -1, z: -1 };

        let first = install_imported_chunk(
            &mut dimension,
            imported(position, &[(-1, State::Stone), (1, State::Dirt)]),
            || section(State::Air),
        )
        .expect("first install");
        let first_handle = first.handle;
        assert_eq!(first_handle.generation, ChunkGeneration(1));
        assert_eq!(
            dimension
                .resolve_chunk(first_handle)
                .expect("first generation")
                .get_block(origin(position, 16)),
            Ok(State::Dirt)
        );

        let unloaded = dimension
            .unload_chunk(first_handle)
            .expect("exact generation unloads");
        assert_eq!(unloaded.get_block(origin(position, -16)), Ok(State::Stone));
        drop(unloaded);
        assert_eq!(dimension.resident_chunk_count(), 0);

        let second = install_imported_chunk(
            &mut dimension,
            imported(position, &[(-1, State::Stone), (1, State::Dirt)]),
            || section(State::Air),
        )
        .expect("reload installs a fresh incarnation");
        assert_eq!(second.handle.generation, ChunkGeneration(2));
        assert!(matches!(
            dimension.resolve_chunk(first_handle),
            Err(ResidentChunkAccessError::StaleGeneration {
                current,
                handle,
                ..
            }) if current == second.handle.generation && handle == first_handle.generation
        ));
        let reloaded = dimension
            .resolve_chunk(second.handle)
            .expect("fresh generation resolves");
        assert_eq!(reloaded.get_block(origin(position, -16)), Ok(State::Stone));
        assert_eq!(reloaded.get_block(origin(position, 0)), Ok(State::Air));
        assert_eq!(reloaded.get_block(origin(position, 16)), Ok(State::Dirt));
        assert!(reloaded.masks_match_recomputation());
    }
}
