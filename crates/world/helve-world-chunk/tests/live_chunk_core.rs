use helve_generated::{AIR, BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts};
use helve_types::{BlockPos, ChunkGeneration, ChunkPos, ChunkRevision, ChunkStamp};
use helve_world_chunk::{ChunkCoreError, LiveChunkCore, MutationFacts, SectionMasks};
use helve_world_contract::{
    BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionStateFacts,
};
use helve_world_reference::DirectBlockSection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    Air,
    Solid,
    Fluid,
    RandomBlock,
    RandomFluid,
}

struct Facts;

impl BlockStateFacts<State> for Facts {
    fn facts(&self, state: State) -> SectionStateFacts {
        match state {
            State::Air => SectionStateFacts::new(false, false, false, false),
            State::Solid => SectionStateFacts::new(true, false, false, false),
            State::Fluid => SectionStateFacts::new(true, true, false, false),
            State::RandomBlock => SectionStateFacts::new(true, false, true, false),
            State::RandomFluid => SectionStateFacts::new(true, true, false, true),
        }
    }
}

type TestChunk = LiveChunkCore<State, DirectBlockSection<State>>;

fn synthetic_chunk(position: ChunkPos, min_section_y: i32, section_count: usize) -> TestChunk {
    let sections = (0..section_count)
        .map(|_| DirectBlockSection::filled(State::Air, &Facts))
        .collect();
    LiveChunkCore::new(position, ChunkGeneration(9), min_section_y, sections)
        .expect("valid synthetic chunk")
}

fn world_pos(chunk: ChunkPos, section_y: i32, local: (u8, u8, u8)) -> BlockPos {
    BlockPos {
        x: chunk.x * 16 + i32::from(local.0),
        y: section_y * 16 + i32::from(local.1),
        z: chunk.z * 16 + i32::from(local.2),
    }
}

#[test]
fn construction_derives_exact_masks() {
    let mut sections = vec![
        DirectBlockSection::filled(State::Air, &Facts),
        DirectBlockSection::filled(State::Solid, &Facts),
        DirectBlockSection::filled(State::Fluid, &Facts),
        DirectBlockSection::filled(State::RandomBlock, &Facts),
        DirectBlockSection::filled(State::RandomFluid, &Facts),
    ];
    let pos = SectionBlockPos::new(1, 2, 3).expect("valid local coordinate");
    sections[0].replace(pos, State::Solid, &Facts);
    let chunk = LiveChunkCore::new(ChunkPos { x: 0, z: 0 }, ChunkGeneration(1), -2, sections)
        .expect("valid chunk");

    assert_eq!(chunk.masks().non_air_bits(), 0b1_1111);
    assert_eq!(chunk.masks().fluid_bits(), 0b1_0100);
    assert_eq!(chunk.masks().random_tick_bits(), 0b1_1000);
    assert_eq!(chunk.masks(), chunk.recompute_masks());
}

#[test]
fn all_air_chunk_has_empty_masks_and_initial_stamp() {
    let chunk = synthetic_chunk(ChunkPos { x: -2, z: 3 }, -4, 24);
    assert_eq!(chunk.masks(), SectionMasks::default());
    assert_eq!(chunk.revision(), ChunkRevision(0));
    assert_eq!(
        chunk.stamp(),
        ChunkStamp {
            generation: ChunkGeneration(9),
            revision: ChunkRevision(0),
        }
    );
}

#[test]
fn affected_section_bits_set_and_clear_exactly() {
    let chunk_pos = ChunkPos { x: 1, z: -2 };
    let mut chunk = synthetic_chunk(chunk_pos, -2, 4);
    let target = world_pos(chunk_pos, -1, (4, 5, 6));

    let first = chunk
        .replace_block(target, State::RandomFluid, &Facts)
        .expect("in-bounds mutation");
    assert_eq!(
        first,
        MutationFacts {
            pos: target,
            old: State::Air,
            new: State::RandomFluid,
            changed: true,
        }
    );
    assert_eq!(chunk.masks().non_air_bits(), 0b0010);
    assert_eq!(chunk.masks().fluid_bits(), 0b0010);
    assert_eq!(chunk.masks().random_tick_bits(), 0b0010);
    assert_eq!(chunk.revision(), ChunkRevision(1));

    let second = chunk
        .replace_block(target, State::Air, &Facts)
        .expect("in-bounds mutation");
    assert!(second.changed);
    assert_eq!(chunk.masks(), SectionMasks::default());
    assert_eq!(chunk.revision(), ChunkRevision(2));
    assert!(chunk.masks_match_recomputation());
}

#[test]
fn same_state_mutation_is_revision_and_mask_stable() {
    let chunk_pos = ChunkPos { x: 0, z: 0 };
    let mut chunk = synthetic_chunk(chunk_pos, 0, 1);
    let target = world_pos(chunk_pos, 0, (15, 15, 15));
    let before = chunk.stamp();
    let before_masks = chunk.masks();

    let facts = chunk
        .replace_block(target, State::Air, &Facts)
        .expect("in-bounds mutation");
    assert!(!facts.changed);
    assert_eq!(facts.old, State::Air);
    assert_eq!(chunk.stamp(), before);
    assert_eq!(chunk.masks(), before_masks);
}

#[test]
fn negative_chunk_coordinates_use_euclidean_mapping() {
    let chunk_pos = ChunkPos { x: -1, z: -2 };
    let mut chunk = synthetic_chunk(chunk_pos, -2, 4);

    for x in -16..=-1 {
        for z in -32..=-17 {
            let pos = BlockPos { x, y: -1, z };
            chunk
                .replace_block(pos, State::Solid, &Facts)
                .expect("negative coordinate belongs to chunk");
            assert_eq!(
                chunk
                    .get_block(pos)
                    .expect("same position remains readable"),
                State::Solid
            );
            chunk
                .replace_block(pos, State::Air, &Facts)
                .expect("negative coordinate remains writable");
        }
    }

    assert_eq!(chunk.masks(), SectionMasks::default());
    assert!(chunk.masks_match_recomputation());
}

#[test]
fn signed_section_boundaries_map_exactly() {
    let chunk_pos = ChunkPos { x: 0, z: 0 };
    let mut chunk = synthetic_chunk(chunk_pos, -2, 5);
    let ys = [-17, -16, -15, -1, 0, 15, 16, 17];

    for (index, y) in ys.into_iter().enumerate() {
        let pos = BlockPos { x: 0, y, z: 0 };
        let state = if index % 2 == 0 {
            State::Solid
        } else {
            State::Fluid
        };
        chunk
            .replace_block(pos, state, &Facts)
            .expect("boundary position belongs to configured lattice");
        assert_eq!(chunk.get_block(pos).expect("boundary read succeeds"), state);
    }
    assert!(chunk.masks_match_recomputation());
}

#[test]
fn wrong_chunk_and_vertical_range_fail_without_mutation() {
    let chunk_pos = ChunkPos { x: -1, z: 2 };
    let mut chunk = synthetic_chunk(chunk_pos, -1, 2);
    let before_stamp = chunk.stamp();
    let before_masks = chunk.masks();

    let wrong_x = BlockPos { x: 0, y: 0, z: 32 };
    assert!(matches!(
        chunk.replace_block(wrong_x, State::Solid, &Facts),
        Err(ChunkCoreError::PositionOutsideChunk { .. })
    ));
    let too_low = world_pos(chunk_pos, -2, (0, 0, 0));
    assert!(matches!(
        chunk.replace_block(too_low, State::Solid, &Facts),
        Err(ChunkCoreError::PositionOutsideVerticalLattice { .. })
    ));
    let too_high = world_pos(chunk_pos, 1, (0, 0, 0));
    assert!(matches!(
        chunk.replace_block(too_high, State::Solid, &Facts),
        Err(ChunkCoreError::PositionOutsideVerticalLattice { .. })
    ));
    assert_eq!(chunk.stamp(), before_stamp);
    assert_eq!(chunk.masks(), before_masks);
}

#[test]
fn invalid_section_lattices_fail_closed() {
    let empty: Vec<DirectBlockSection<State>> = Vec::new();
    assert!(matches!(
        LiveChunkCore::new(ChunkPos { x: 0, z: 0 }, ChunkGeneration(1), 0, empty),
        Err(ChunkCoreError::EmptySectionLattice)
    ));

    let too_many = (0..65)
        .map(|_| DirectBlockSection::filled(State::Air, &Facts))
        .collect();
    assert!(matches!(
        LiveChunkCore::new(ChunkPos { x: 0, z: 0 }, ChunkGeneration(1), 0, too_many),
        Err(ChunkCoreError::SectionCountExceedsMaskCapacity { count: 65 })
    ));

    let one = vec![DirectBlockSection::filled(State::Air, &Facts)];
    assert!(matches!(
        LiveChunkCore::new(ChunkPos { x: 0, z: 0 }, ChunkGeneration(1), i32::MAX, one),
        Err(ChunkCoreError::SectionRangeOverflow)
    ));
}

fn target_expected_masks(states: &[BlockStateId], section_count: usize) -> (u64, u64, u64) {
    let facts = GeneratedStateFacts;
    let mut non_air = 0_u64;
    let mut fluid = 0_u64;
    let mut random = 0_u64;

    for section in 0..section_count {
        let start = section * BLOCK_SECTION_CELLS;
        let end = start + BLOCK_SECTION_CELLS;
        let mut section_non_air = false;
        let mut section_fluid = false;
        let mut section_random = false;
        for &state in &states[start..end] {
            let state_facts = facts.facts(state);
            section_non_air |= state_facts.non_air();
            section_fluid |= state_facts.counted_fluid();
            section_random |= state_facts.random_block() || state_facts.random_fluid();
        }
        let bit = 1_u64 << section;
        if section_non_air {
            non_air |= bit;
        }
        if section_fluid {
            fluid |= bit;
        }
        if section_random {
            random |= bit;
        }
    }

    (non_air, fluid, random)
}

#[test]
fn hundred_thousand_target_mutations_match_independent_image_and_masks() {
    const SECTION_COUNT: usize = 8;
    const OPERATIONS: usize = 100_000;
    const BARRIER: usize = 997;

    let facts = GeneratedStateFacts;
    let chunk_pos = ChunkPos { x: -3, z: 2 };
    let min_section_y = -4;
    let sections = (0..SECTION_COUNT)
        .map(|_| DirectBlockSection::filled(AIR, &facts))
        .collect();
    let mut chunk = LiveChunkCore::new(chunk_pos, ChunkGeneration(77), min_section_y, sections)
        .expect("valid target chunk");
    let mut expected = vec![AIR; SECTION_COUNT * BLOCK_SECTION_CELLS];
    let mut expected_revision = 0_u64;
    let mut rng = 0xC4A3_5EED_19D0_4A71_u64;
    let state_count = u64::try_from(BLOCK_STATE_COUNT).expect("target state count fits u64");
    let section_count = u64::try_from(SECTION_COUNT).expect("section count fits u64");

    for operation in 0..OPERATIONS {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let local_x = u8::try_from(rng & 15).expect("masked x fits u8");
        let local_z = u8::try_from((rng >> 4) & 15).expect("masked z fits u8");
        let local_y = u8::try_from((rng >> 8) & 15).expect("masked y fits u8");
        let section_index =
            usize::try_from((rng >> 12) % section_count).expect("section index fits usize");
        let raw_state = u32::try_from((rng >> 20) % state_count).expect("target state id fits u32");
        let state = BlockStateId::new(raw_state).expect("generated target state is in range");
        let section_y =
            min_section_y + i32::try_from(section_index).expect("small section index fits i32");
        let pos = world_pos(chunk_pos, section_y, (local_x, local_y, local_z));
        let local = SectionBlockPos::new(local_x, local_y, local_z).expect("valid local pos");
        let expected_index = section_index * BLOCK_SECTION_CELLS + local.index();
        let old = expected[expected_index];

        let mutation = chunk
            .replace_block(pos, state, &facts)
            .expect("generated position remains in chunk");
        assert_eq!(mutation.old, old);
        assert_eq!(mutation.new, state);
        assert_eq!(mutation.changed, old != state);
        if old != state {
            expected[expected_index] = state;
            expected_revision += 1;
        }
        assert_eq!(chunk.revision(), ChunkRevision(expected_revision));

        if operation % BARRIER == 0 || operation + 1 == OPERATIONS {
            let (non_air, fluid, random) = target_expected_masks(&expected, SECTION_COUNT);
            assert_eq!(chunk.masks().non_air_bits(), non_air);
            assert_eq!(chunk.masks().fluid_bits(), fluid);
            assert_eq!(chunk.masks().random_tick_bits(), random);
            assert!(chunk.masks_match_recomputation());

            for section in 0..SECTION_COUNT {
                let section_y =
                    min_section_y + i32::try_from(section).expect("small section index fits i32");
                for cell in 0..BLOCK_SECTION_CELLS {
                    let x = u8::try_from(cell & 15).expect("cell x fits u8");
                    let z = u8::try_from((cell >> 4) & 15).expect("cell z fits u8");
                    let y = u8::try_from((cell >> 8) & 15).expect("cell y fits u8");
                    let pos = world_pos(chunk_pos, section_y, (x, y, z));
                    assert_eq!(
                        chunk.get_block(pos).expect("barrier read remains in chunk"),
                        expected[section * BLOCK_SECTION_CELLS + cell]
                    );
                }
            }
        }
    }
}
