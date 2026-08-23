use crucible_types::{BlockPos, ChunkGeneration, ChunkPos};
use crucible_world_chunk::LiveChunkCore;
use crucible_world_contract::{
    BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionStateFacts,
    SectionSummary,
};
use crucible_world_reference::DirectBlockSection;

struct Facts;

impl BlockStateFacts<u16> for Facts {
    fn facts(&self, state: u16) -> SectionStateFacts {
        let non_air = state != 0;
        SectionStateFacts::new(
            non_air,
            non_air && state.is_multiple_of(7),
            non_air && state.is_multiple_of(11),
            non_air && state.is_multiple_of(13),
        )
    }
}

#[derive(Debug)]
struct ScanSection {
    cells: Box<[u16; BLOCK_SECTION_CELLS]>,
}

impl ScanSection {
    fn filled(state: u16) -> Self {
        Self {
            cells: Box::new([state; BLOCK_SECTION_CELLS]),
        }
    }
}

impl BlockSection<u16> for ScanSection {
    fn get(&self, pos: SectionBlockPos) -> u16 {
        self.cells[pos.index()]
    }

    fn replace<F: BlockStateFacts<u16>>(
        &mut self,
        pos: SectionBlockPos,
        state: u16,
        _facts: &F,
    ) -> u16 {
        let previous = self.cells[pos.index()];
        self.cells[pos.index()] = state;
        previous
    }

    fn summary(&self) -> SectionSummary {
        let mut summary = SectionSummary::default();
        for &state in self.cells.iter() {
            let facts = Facts.facts(state);
            summary.non_air_count += u16::from(facts.non_air());
            summary.fluid_count += u16::from(facts.counted_fluid());
            summary.random_block_present |= facts.random_block();
            summary.random_fluid_present |= facts.random_fluid();
        }
        summary
    }

    fn maybe_contains<P: FnMut(u16) -> bool>(&self, predicate: P) -> bool {
        self.cells.iter().copied().any(predicate)
    }
}

fn section_pos(index: usize) -> SectionBlockPos {
    let x = u8::try_from(index & 0x0f).expect("masked x fits u8");
    let z = u8::try_from((index >> 4) & 0x0f).expect("masked z fits u8");
    let y = u8::try_from((index >> 8) & 0x0f).expect("masked y fits u8");
    SectionBlockPos::new(x, y, z).expect("decoded section index is valid")
}

fn world_pos(chunk: ChunkPos, min_section_y: i32, section_index: usize, index: usize) -> BlockPos {
    let local = section_pos(index);
    let section_offset = i32::try_from(section_index).expect("test section index fits i32");
    BlockPos {
        x: chunk.x * 16 + i32::from(local.x()),
        y: (min_section_y + section_offset) * 16 + i32::from(local.y()),
        z: chunk.z * 16 + i32::from(local.z()),
    }
}

#[test]
fn publication_is_canonical_and_matches_live_reads() {
    let position = ChunkPos { x: -2, z: 3 };
    let min_section_y = -4;
    let sections = (0..3)
        .map(|_| DirectBlockSection::filled(0_u16, &Facts))
        .collect::<Vec<_>>();
    let mut chunk = LiveChunkCore::new(position, ChunkGeneration(9), min_section_y, sections)
        .expect("valid live chunk");

    for section_index in 0..3 {
        for index in 0..BLOCK_SECTION_CELLS {
            let state = u16::try_from(section_index * BLOCK_SECTION_CELLS + index + 1)
                .expect("test state fits u16");
            chunk
                .replace_block(
                    world_pos(position, min_section_y, section_index, index),
                    state,
                    &Facts,
                )
                .expect("in-range mutation");
        }
    }

    let publication = chunk.publish_semantic_image();
    assert_eq!(publication.position(), position);
    assert_eq!(publication.stamp(), chunk.stamp());
    assert_eq!(publication.min_section_y(), min_section_y);
    assert_eq!(publication.section_count(), 3);
    assert_eq!(publication.masks(), chunk.masks());
    assert_eq!(publication.states().len(), 3 * BLOCK_SECTION_CELLS);
    assert!(chunk.is_publication_current(&publication));

    for section_index in 0..3 {
        let section = publication
            .section_states(section_index)
            .expect("published section exists");
        for index in 0..BLOCK_SECTION_CELLS {
            let expected = u16::try_from(section_index * BLOCK_SECTION_CELLS + index + 1)
                .expect("test state fits u16");
            let local = section_pos(index);
            assert_eq!(section[index], expected);
            assert_eq!(publication.get(section_index, local), Some(expected));
            assert_eq!(
                chunk
                    .get_block(world_pos(position, min_section_y, section_index, index))
                    .expect("published cell is live-readable"),
                expected
            );
        }
    }
    assert!(publication.section_states(3).is_none());
    assert!(publication.get(3, section_pos(0)).is_none());
}

#[test]
fn publication_freshness_is_position_generation_and_revision_bound() {
    let position = ChunkPos { x: 4, z: -7 };
    let sections = vec![DirectBlockSection::filled(0_u16, &Facts)];
    let mut chunk =
        LiveChunkCore::new(position, ChunkGeneration(20), -2, sections).expect("valid live chunk");
    let publication = chunk.publish_semantic_image();
    let original_stamp = publication.stamp();

    let first = world_pos(position, -2, 0, 0);
    chunk
        .replace_block(first, 0, &Facts)
        .expect("same-state mutation");
    assert_eq!(chunk.stamp(), original_stamp);
    assert!(chunk.accepts_stamp(original_stamp));
    assert!(chunk.is_publication_current(&publication));

    chunk
        .replace_block(first, 1, &Facts)
        .expect("real mutation");
    assert!(!chunk.accepts_stamp(original_stamp));
    assert!(!chunk.is_publication_current(&publication));
    assert_eq!(publication.get(0, section_pos(0)), Some(0));

    let other_generation = LiveChunkCore::new(
        position,
        ChunkGeneration(21),
        -2,
        vec![DirectBlockSection::filled(0_u16, &Facts)],
    )
    .expect("replacement generation");
    assert_eq!(other_generation.revision().0, 0);
    assert!(!other_generation.is_publication_current(&publication));

    let other_position = LiveChunkCore::new(
        ChunkPos { x: 5, z: -7 },
        ChunkGeneration(20),
        -2,
        vec![DirectBlockSection::filled(0_u16, &Facts)],
    )
    .expect("other chunk position");
    assert!(!other_position.is_publication_current(&publication));
}

#[test]
fn publication_does_not_depend_on_section_backing_representation() {
    let position = ChunkPos { x: -1, z: -1 };
    let min_section_y = -4;
    let mut direct = LiveChunkCore::new(
        position,
        ChunkGeneration(31),
        min_section_y,
        vec![
            DirectBlockSection::filled(0_u16, &Facts),
            DirectBlockSection::filled(0_u16, &Facts),
        ],
    )
    .expect("direct chunk");
    let mut scan = LiveChunkCore::new(
        position,
        ChunkGeneration(31),
        min_section_y,
        vec![ScanSection::filled(0), ScanSection::filled(0)],
    )
    .expect("scan chunk");

    for operation in 0..2_000_usize {
        let section_index = operation & 1;
        let index = operation.wrapping_mul(2_653) & (BLOCK_SECTION_CELLS - 1);
        let state = u16::try_from((operation % 503) + 1).expect("bounded test state");
        let pos = world_pos(position, min_section_y, section_index, index);
        direct
            .replace_block(pos, state, &Facts)
            .expect("direct mutation");
        scan.replace_block(pos, state, &Facts)
            .expect("scan mutation");
    }

    assert_eq!(direct.stamp(), scan.stamp());
    assert_eq!(direct.masks(), scan.masks());
    assert_eq!(
        direct.publish_semantic_image(),
        scan.publish_semantic_image()
    );
}

#[test]
fn long_mutation_trace_keeps_publications_exact_and_immutable() {
    let position = ChunkPos { x: -3, z: 2 };
    let min_section_y = -4;
    let mut chunk = LiveChunkCore::new(
        position,
        ChunkGeneration(44),
        min_section_y,
        vec![
            DirectBlockSection::filled(0_u16, &Facts),
            DirectBlockSection::filled(0_u16, &Facts),
        ],
    )
    .expect("trace chunk");
    let mut expected = vec![0_u16; 2 * BLOCK_SECTION_CELLS];
    let expected_len = u64::try_from(expected.len()).expect("test image length fits u64");
    let mut rng = 0xD1B5_4A32_D192_ED03_u64;

    for operation in 0..20_000_usize {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let flat_index =
            usize::try_from(rng % expected_len).expect("bounded trace index fits usize");
        let section_index = flat_index / BLOCK_SECTION_CELLS;
        let cell_index = flat_index % BLOCK_SECTION_CELLS;
        let state = u16::try_from(((rng >> 32) % 2_048) + 1).expect("bounded trace state");
        let pos = world_pos(position, min_section_y, section_index, cell_index);
        chunk
            .replace_block(pos, state, &Facts)
            .expect("trace mutation");
        expected[flat_index] = state;

        if operation.is_multiple_of(257) {
            let publication = chunk.publish_semantic_image();
            assert_eq!(publication.states(), expected.as_slice());
            assert_eq!(publication.stamp(), chunk.stamp());
            assert_eq!(publication.masks(), chunk.recompute_masks());
            assert!(chunk.is_publication_current(&publication));

            for (index, &published_state) in publication.states().iter().enumerate() {
                let published_section = index / BLOCK_SECTION_CELLS;
                let published_cell = index % BLOCK_SECTION_CELLS;
                let live = chunk
                    .get_block(world_pos(
                        position,
                        min_section_y,
                        published_section,
                        published_cell,
                    ))
                    .expect("published cell is live-readable");
                assert_eq!(published_state, live);
            }

            let frozen_first = publication.states()[0];
            let changed = match frozen_first {
                1 => 2,
                _ => 1,
            };
            chunk
                .replace_block(world_pos(position, min_section_y, 0, 0), changed, &Facts)
                .expect("post-publication mutation");
            expected[0] = changed;
            assert_eq!(publication.states()[0], frozen_first);
            assert!(!chunk.is_publication_current(&publication));
        }
    }
}
