use helve_generated::{BlockStateId, GeneratedStateFacts};
use helve_types::{BlockPos, ChunkPos, DimensionId, DimensionTypeId};
use helve_world_reference::DirectBlockSection;
use helve_world_runtime::{
    DimensionInstance, DimensionRuntimeProfile, ResidentChunkAccessError, ResidentChunkHandle,
};

const MIN_BLOCK_Y: i32 = -64;
const HEIGHT: u32 = 384;
const BLOCKS_PER_CHUNK_AXIS: i32 = 16;

type Section = DirectBlockSection<BlockStateId>;
type Dimension = DimensionInstance<BlockStateId, Section>;

fn profile() -> DimensionRuntimeProfile {
    DimensionRuntimeProfile::new(DimensionTypeId(1), MIN_BLOCK_Y, HEIGHT, true)
        .expect("standard 24-section profile")
}

fn state(raw: u32) -> BlockStateId {
    BlockStateId::new(raw).expect("test state is inside generated 26.2 universe")
}

fn sections(profile: DimensionRuntimeProfile, value: BlockStateId) -> Vec<Section> {
    (0..profile.section_count())
        .map(|_| DirectBlockSection::filled(value, &GeneratedStateFacts))
        .collect()
}

fn probe(position: ChunkPos, y: i32) -> BlockPos {
    BlockPos {
        x: position.x * BLOCKS_PER_CHUNK_AXIS + 3,
        y,
        z: position.z * BLOCKS_PER_CHUNK_AXIS + 11,
    }
}

#[test]
fn generated_state_grid_survives_load_mutate_unload_reload() {
    let profile = profile();
    let mut dimension = Dimension::with_chunk_capacity(DimensionId(17), profile, 9);
    let positions = (-1..=1)
        .flat_map(|z| (-1..=1).map(move |x| ChunkPos { x, z }))
        .collect::<Vec<_>>();

    let mut first_handles = Vec::with_capacity(positions.len());
    for (index, position) in positions.iter().copied().enumerate() {
        let initial = state(u32::try_from(index + 1).expect("small state identity"));
        let handle = dimension
            .load_chunk(position, sections(profile, initial))
            .expect("first resident epoch loads");
        assert_eq!(dimension.discover_chunk(position), Some(handle));
        assert_eq!(
            dimension
                .resolve_chunk(handle)
                .expect("fresh handle resolves")
                .get_block(probe(position, 64))
                .expect("probe lies in chunk"),
            initial
        );
        first_handles.push(handle);
    }
    assert_eq!(dimension.resident_chunk_count(), positions.len());

    let center = first_handles[4];
    let mutation_pos = probe(center.position, 70);
    let replacement = state(31);
    let mutation = dimension
        .resolve_chunk_mut(center)
        .expect("center handle resolves mutably")
        .replace_block(mutation_pos, replacement, &GeneratedStateFacts)
        .expect("generated-state mutation succeeds");
    assert!(mutation.changed);
    assert_eq!(mutation.new, replacement);
    let center_chunk = dimension
        .resolve_chunk(center)
        .expect("center remains resident after mutation");
    assert_eq!(center_chunk.revision().0, 1);
    assert!(center_chunk.masks_match_recomputation());
    assert_eq!(
        center_chunk
            .get_block(mutation_pos)
            .expect("mutated position remains readable"),
        replacement
    );

    for handle in first_handles.iter().copied().rev() {
        let unloaded = dimension
            .unload_chunk(handle)
            .expect("first epoch unload succeeds");
        assert_eq!(unloaded.position(), handle.position);
        assert_eq!(unloaded.generation(), handle.generation);
        assert!(unloaded.masks_match_recomputation());
        if handle == center {
            assert_eq!(unloaded.revision().0, 1);
            assert_eq!(
                unloaded
                    .get_block(mutation_pos)
                    .expect("unloaded semantic state is returned to caller"),
                replacement
            );
        }
    }
    assert_eq!(dimension.resident_chunk_count(), 0);

    let mut second_handles = Vec::with_capacity(positions.len());
    for (index, position) in positions.iter().copied().enumerate() {
        let replacement_epoch_state =
            state(u32::try_from(index + 101).expect("small replacement-epoch state identity"));
        let handle = dimension
            .load_chunk(position, sections(profile, replacement_epoch_state))
            .expect("second resident epoch loads");
        second_handles.push((handle, replacement_epoch_state));
    }

    for (old, (current, expected_state)) in first_handles
        .iter()
        .copied()
        .zip(second_handles.iter().copied())
    {
        assert_eq!(old.position, current.position);
        assert_ne!(old.generation, current.generation);
        assert!(matches!(
            dimension.resolve_chunk(old),
            Err(ResidentChunkAccessError::StaleGeneration {
                position,
                current: live,
                handle: stale,
            }) if position == current.position && live == current.generation && stale == old.generation
        ));
        assert_eq!(
            dimension
                .resolve_chunk(current)
                .expect("replacement handle resolves")
                .get_block(probe(current.position, 64))
                .expect("replacement probe lies in chunk"),
            expected_state
        );
    }
}

#[test]
fn stale_handle_never_aliases_repeated_reloads() {
    let profile = profile();
    let position = ChunkPos { x: -37, z: 22 };
    let mut dimension = Dimension::new(DimensionId(23), profile);
    let mut previous: Option<ResidentChunkHandle> = None;

    for epoch in 0_u32..64 {
        let current = dimension
            .load_chunk(position, sections(profile, state(epoch + 1)))
            .expect("epoch loads");
        if let Some(stale) = previous {
            assert_ne!(stale.generation, current.generation);
            assert!(matches!(
                dimension.resolve_chunk(stale),
                Err(ResidentChunkAccessError::StaleGeneration { .. })
            ));
        }
        assert_eq!(dimension.discover_chunk(position), Some(current));
        let unloaded = dimension
            .unload_chunk(current)
            .expect("epoch unload succeeds");
        assert_eq!(unloaded.generation(), current.generation);
        assert_eq!(dimension.resident_chunk_count(), 0);
        previous = Some(current);
    }
}
