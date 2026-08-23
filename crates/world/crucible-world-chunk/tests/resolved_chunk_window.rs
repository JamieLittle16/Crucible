use std::collections::HashMap;

use crucible_types::{BlockPos, ChunkGeneration, ChunkPos};
use crucible_world_chunk::{
    ChunkCoreError, LiveChunkCore, ResolvedChunkWindow, ResolvedChunkWindowError,
};
use crucible_world_contract::{BlockStateFacts, SectionStateFacts};
use crucible_world_reference::DirectBlockSection;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct State(u16);

struct Facts;

impl BlockStateFacts<State> for Facts {
    fn facts(&self, state: State) -> SectionStateFacts {
        SectionStateFacts::new(state.0 != 0, false, false, false)
    }
}

type Chunk = LiveChunkCore<State, DirectBlockSection<State>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SemanticRead {
    State(State),
    MissingHorizontalCoverage,
    OutsideVerticalLattice,
}

struct ReferencePointLookup<'a> {
    chunks: HashMap<ChunkPos, &'a Chunk>,
}

impl<'a> ReferencePointLookup<'a> {
    fn new(chunks: impl IntoIterator<Item = &'a Chunk>) -> Self {
        Self {
            chunks: chunks
                .into_iter()
                .map(|chunk| (chunk.position(), chunk))
                .collect(),
        }
    }

    fn get_block(&self, pos: BlockPos) -> SemanticRead {
        let chunk_pos = ChunkPos {
            x: pos.x.div_euclid(16),
            z: pos.z.div_euclid(16),
        };
        let Some(chunk) = self.chunks.get(&chunk_pos) else {
            return SemanticRead::MissingHorizontalCoverage;
        };
        match chunk.get_block(pos) {
            Ok(state) => SemanticRead::State(state),
            Err(ChunkCoreError::PositionOutsideVerticalLattice { .. }) => {
                SemanticRead::OutsideVerticalLattice
            }
            Err(error) => panic!("reference routing produced impossible chunk error: {error:?}"),
        }
    }
}

fn window_read(
    window: &ResolvedChunkWindow<'_, State, DirectBlockSection<State>>,
    pos: BlockPos,
) -> SemanticRead {
    match window.get_block(pos) {
        Ok(state) => SemanticRead::State(state),
        Err(ResolvedChunkWindowError::PositionOutsideWindow { .. }) => {
            SemanticRead::MissingHorizontalCoverage
        }
        Err(ResolvedChunkWindowError::Chunk(ChunkCoreError::PositionOutsideVerticalLattice {
            ..
        })) => SemanticRead::OutsideVerticalLattice,
        Err(error) => panic!("resolved window produced unexpected read error: {error:?}"),
    }
}

fn state_for(position: ChunkPos, section_index: usize) -> State {
    let section_index = i64::try_from(section_index).expect("small test section index fits i64");
    let mixed = (i64::from(position.x) * 131 + i64::from(position.z) * 977 + section_index * 17)
        .rem_euclid(60_000);
    State(u16::try_from(mixed + 1).expect("test state identity fits u16"))
}

fn synthetic_chunk(position: ChunkPos, min_section_y: i32, section_count: usize) -> Chunk {
    let sections = (0..section_count)
        .map(|index| DirectBlockSection::filled(state_for(position, index), &Facts))
        .collect();
    LiveChunkCore::new(position, ChunkGeneration(1), min_section_y, sections)
        .expect("synthetic chunk lattice is valid")
}

fn chunk_grid(
    origin: ChunkPos,
    width: usize,
    depth: usize,
    min_section_y: i32,
    section_count: usize,
) -> Vec<Chunk> {
    let mut chunks = Vec::with_capacity(width * depth);
    for z in 0..depth {
        for x in 0..width {
            let x = i32::try_from(x).expect("small test width fits i32");
            let z = i32::try_from(z).expect("small test depth fits i32");
            chunks.push(synthetic_chunk(
                ChunkPos {
                    x: origin.x + x,
                    z: origin.z + z,
                },
                min_section_y,
                section_count,
            ));
        }
    }
    chunks
}

#[test]
fn one_chunk_window_reads_exact_semantic_image() {
    let chunks = chunk_grid(ChunkPos { x: -1, z: -1 }, 1, 1, -1, 2);
    let window = ResolvedChunkWindow::new(ChunkPos { x: -1, z: -1 }, 1, 1, chunks.iter())
        .expect("complete one-chunk window");

    assert_eq!(window.origin(), ChunkPos { x: -1, z: -1 });
    assert_eq!(window.width(), 1);
    assert_eq!(window.depth(), 1);
    assert_eq!(window.chunk_count(), 1);

    for pos in [
        BlockPos {
            x: -16,
            y: -16,
            z: -16,
        },
        BlockPos {
            x: -1,
            y: -1,
            z: -1,
        },
        BlockPos {
            x: -16,
            y: 0,
            z: -1,
        },
        BlockPos {
            x: -1,
            y: 15,
            z: -16,
        },
    ] {
        let expected = chunks[0]
            .get_block(pos)
            .expect("test point lies in the one-chunk lattice");
        assert_eq!(window_read(&window, pos), SemanticRead::State(expected));
    }
}

#[test]
fn multi_chunk_window_accepts_arbitrary_input_order_and_crosses_signed_boundaries() {
    let origin = ChunkPos { x: -2, z: -1 };
    let chunks = chunk_grid(origin, 4, 3, -2, 4);
    let reference = ReferencePointLookup::new(chunks.iter());
    let window = ResolvedChunkWindow::new(origin, 4, 3, chunks.iter().rev())
        .expect("complete shuffled window");

    let xs = [-32, -17, -16, -1, 0, 15, 16, 31];
    let zs = [-16, -1, 0, 15, 16, 31, 32, 47];
    let ys = [-32, -17, -16, -1, 0, 15, 16, 31];
    for x in xs {
        for z in zs {
            for y in ys {
                let pos = BlockPos { x, y, z };
                assert_eq!(
                    window_read(&window, pos),
                    reference.get_block(pos),
                    "{pos:?}"
                );
            }
        }
    }
}

#[test]
fn construction_rejects_empty_overflow_missing_duplicate_and_outside_sets() {
    let origin = ChunkPos { x: -1, z: 2 };
    let chunks = chunk_grid(origin, 2, 2, -1, 2);

    assert!(matches!(
        ResolvedChunkWindow::new(origin, 0, 2, chunks.iter()),
        Err(ResolvedChunkWindowError::EmptyExtent)
    ));
    assert!(matches!(
        ResolvedChunkWindow::new(origin, usize::MAX, 2, chunks.iter()),
        Err(ResolvedChunkWindowError::ExtentOverflow)
    ));

    let missing = ResolvedChunkWindow::new(origin, 2, 2, chunks.iter().take(3));
    assert!(matches!(
        missing,
        Err(ResolvedChunkWindowError::MissingChunk {
            position: ChunkPos { x: 0, z: 3 },
        })
    ));

    let duplicate = ResolvedChunkWindow::new(
        origin,
        2,
        2,
        [&chunks[0], &chunks[1], &chunks[2], &chunks[2], &chunks[3]],
    );
    assert!(matches!(
        duplicate,
        Err(ResolvedChunkWindowError::DuplicateChunk { position })
            if position == chunks[2].position()
    ));

    let outside_chunk = synthetic_chunk(ChunkPos { x: 7, z: 7 }, -1, 2);
    let outside = ResolvedChunkWindow::new(
        origin,
        2,
        2,
        [
            &chunks[0],
            &chunks[1],
            &chunks[2],
            &chunks[3],
            &outside_chunk,
        ],
    );
    assert!(matches!(
        outside,
        Err(ResolvedChunkWindowError::ChunkOutsideWindow {
            position: ChunkPos { x: 7, z: 7 },
            origin: error_origin,
            width: 2,
            depth: 2,
        }) if error_origin == origin
    ));
}

#[test]
fn read_errors_fail_closed_for_horizontal_and_vertical_misses() {
    let origin = ChunkPos { x: -1, z: -1 };
    let chunks = chunk_grid(origin, 2, 2, -1, 2);
    let window =
        ResolvedChunkWindow::new(origin, 2, 2, chunks.iter()).expect("complete resolved window");

    let horizontal = BlockPos { x: 16, y: 0, z: 0 };
    assert!(matches!(
        window.get_block(horizontal),
        Err(ResolvedChunkWindowError::PositionOutsideWindow {
            pos,
            origin: error_origin,
            width: 2,
            depth: 2,
        }) if pos == horizontal && error_origin == origin
    ));

    let vertical = BlockPos {
        x: -1,
        y: 16,
        z: -1,
    };
    assert!(matches!(
        window.get_block(vertical),
        Err(ResolvedChunkWindowError::Chunk(
            ChunkCoreError::PositionOutsideVerticalLattice {
                pos,
                min_section_y: -1,
                section_count: 2,
            }
        )) if pos == vertical
    ));
}

#[test]
fn hundred_thousand_queries_match_reference_router_exactly() {
    let origin = ChunkPos { x: -3, z: -2 };
    let width = 6;
    let depth = 5;
    let min_section_y = -3;
    let section_count = 6;
    let chunks = chunk_grid(origin, width, depth, min_section_y, section_count);
    let reference = ReferencePointLookup::new(chunks.iter());
    let window = ResolvedChunkWindow::new(origin, width, depth, chunks.iter().rev())
        .expect("complete resolved test window");

    let min_x = origin.x * 16;
    let min_z = origin.z * 16;
    let in_width = i32::try_from(width * 16).expect("test width fits i32");
    let in_depth = i32::try_from(depth * 16).expect("test depth fits i32");
    let min_y = min_section_y * 16;
    let in_height = i32::try_from(section_count * 16).expect("test height fits i32");
    let in_width_u64 = u64::try_from(in_width).expect("positive test width fits u64");
    let in_depth_u64 = u64::try_from(in_depth).expect("positive test depth fits u64");
    let in_height_u64 = u64::try_from(in_height).expect("positive test height fits u64");

    let mut rng = 0xD1B5_4A32_D192_ED03_u64;
    for query_index in 0..100_000_u32 {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;

        let mode = query_index % 19;
        let x = if mode == 0 {
            min_x - 1
        } else if mode == 1 {
            min_x + in_width
        } else {
            min_x + i32::try_from(rng % in_width_u64).expect("bounded X offset fits i32")
        };
        let z = if mode == 2 {
            min_z - 1
        } else if mode == 3 {
            min_z + in_depth
        } else {
            min_z + i32::try_from((rng >> 11) % in_depth_u64).expect("bounded Z offset fits i32")
        };
        let y = if mode == 4 {
            min_y - 1
        } else if mode == 5 {
            min_y + in_height
        } else {
            min_y + i32::try_from((rng >> 23) % in_height_u64).expect("bounded Y offset fits i32")
        };
        let pos = BlockPos { x, y, z };

        assert_eq!(
            window_read(&window, pos),
            reference.get_block(pos),
            "query {query_index} at {pos:?}"
        );
    }
}
