use std::collections::BTreeSet;

use crucible_generated::{AIR, BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_MUTATION_FLAGS};
use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection, SectionBlockPos};

use crate::model::{BENCH_SEED, BenchSection, CARDINALITIES, CaseSpec, Mode, Pattern};

const NON_AIR: u8 = 1;
const COUNTED_FLUID: u8 = 2;

#[derive(Clone, Debug)]
pub(crate) struct Prepared<C> {
    pub(crate) section: C,
    pub(crate) actual_cardinality: usize,
    pub(crate) states: Vec<BlockStateId>,
    pub(crate) present_states: Vec<BlockStateId>,
    pub(crate) construction_logical_allocations: usize,
    pub(crate) construction_transitions: usize,
    pub(crate) peak_owned_bytes: usize,
}

pub(crate) fn cases_for(mode: Mode) -> Vec<CaseSpec> {
    let cardinalities: &[usize] = match mode {
        Mode::Smoke => &[1, 16, 17, 256, 257, 4096],
        Mode::Qualification => &CARDINALITIES,
    };
    let mut cases = cardinalities
        .iter()
        .copied()
        .map(|pool_cardinality| CaseSpec {
            pattern: Pattern::CardinalitySpread,
            pool_cardinality,
        })
        .collect::<Vec<_>>();

    let spatial: &[CaseSpec] = match mode {
        Mode::Smoke => &[
            CaseSpec {
                pattern: Pattern::Layered,
                pool_cardinality: 8,
            },
            CaseSpec {
                pattern: Pattern::FluidContaining,
                pool_cardinality: 8,
            },
            CaseSpec {
                pattern: Pattern::SurvivalLike,
                pool_cardinality: 32,
            },
            CaseSpec {
                pattern: Pattern::Noisy,
                pool_cardinality: 257,
            },
        ],
        Mode::Qualification => &[
            CaseSpec {
                pattern: Pattern::Homogeneous,
                pool_cardinality: 1,
            },
            CaseSpec {
                pattern: Pattern::Layered,
                pool_cardinality: 8,
            },
            CaseSpec {
                pattern: Pattern::Clustered,
                pool_cardinality: 16,
            },
            CaseSpec {
                pattern: Pattern::Checker,
                pool_cardinality: 2,
            },
            CaseSpec {
                pattern: Pattern::Noisy,
                pool_cardinality: 257,
            },
            CaseSpec {
                pattern: Pattern::Noisy,
                pool_cardinality: 4096,
            },
            CaseSpec {
                pattern: Pattern::FluidContaining,
                pool_cardinality: 8,
            },
            CaseSpec {
                pattern: Pattern::SurvivalLike,
                pool_cardinality: 32,
            },
            CaseSpec {
                pattern: Pattern::BuildLike,
                pool_cardinality: 64,
            },
        ],
    };
    cases.extend_from_slice(spatial);
    cases
}

pub(crate) fn prepare<C: BenchSection>(case: CaseSpec) -> Prepared<C> {
    assert!((1..=BLOCK_SECTION_CELLS).contains(&case.pool_cardinality));
    let states = state_pool(case);
    let mut section = C::filled(states[0]);
    let mut seen = vec![false; states.len()];
    let mut rng = BENCH_SEED
        ^ u64::try_from(case.pool_cardinality)
            .expect("section cardinality fits u64")
            .rotate_left(17);
    let mut construction_logical_allocations = C::initial_logical_allocations();
    let mut construction_transitions = 0;
    let mut peak_owned_bytes = section.owned_bytes();

    for cell in 0..BLOCK_SECTION_CELLS {
        let state_index = pattern_state_index(case.pattern, cell, states.len(), &mut rng);
        seen[state_index] = true;
        let before = section.representation_name();
        let _ = section.replace(pos(cell), states[state_index], &GeneratedStateFacts);
        let after = section.representation_name();
        if before != after {
            construction_transitions += 1;
            construction_logical_allocations += C::transition_logical_allocations(&before, &after);
        }
        peak_owned_bytes = peak_owned_bytes.max(section.owned_bytes());
    }

    let present_states = seen
        .iter()
        .enumerate()
        .filter_map(|(index, present)| present.then_some(states[index]))
        .collect::<Vec<_>>();
    let actual_cardinality = present_states.len();

    Prepared {
        section,
        actual_cardinality,
        states,
        present_states,
        construction_logical_allocations,
        construction_transitions,
        peak_owned_bytes,
    }
}

fn state_pool(case: CaseSpec) -> Vec<BlockStateId> {
    match case.pattern {
        Pattern::SurvivalLike => survival_state_pool(case.pool_cardinality),
        Pattern::FluidContaining => fluid_state_pool(case.pool_cardinality),
        _ => generic_state_pool(case.pool_cardinality),
    }
}

fn generic_state_pool(cardinality: usize) -> Vec<BlockStateId> {
    (1..=cardinality).map(state_id).collect()
}

fn survival_state_pool(cardinality: usize) -> Vec<BlockStateId> {
    assert!(cardinality >= 2, "survival-like requires AIR plus solid state");
    let mut states = Vec::with_capacity(cardinality);
    states.push(AIR);
    states.extend(states_with_exact_flags(NON_AIR, cardinality - 1));
    states
}

fn fluid_state_pool(cardinality: usize) -> Vec<BlockStateId> {
    assert!(cardinality >= 3, "fluid workload requires air, fluid, and solid");
    let mut states = Vec::with_capacity(cardinality);
    states.push(AIR);
    states.push(
        first_state_matching(|flags| flags & COUNTED_FLUID != 0)
            .expect("qualified target contains counted-fluid states"),
    );
    for state in states_with_exact_flags(NON_AIR, cardinality) {
        if states.len() == cardinality {
            break;
        }
        if !states.contains(&state) {
            states.push(state);
        }
    }
    assert_eq!(states.len(), cardinality);
    states
}

fn states_with_exact_flags(flags: u8, count: usize) -> Vec<BlockStateId> {
    STATE_MUTATION_FLAGS
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, candidate_flags)| *candidate_flags == flags)
        .take(count)
        .map(|(raw, _)| state_id(raw))
        .collect::<Vec<_>>()
}

fn first_state_matching(mut predicate: impl FnMut(u8) -> bool) -> Option<BlockStateId> {
    STATE_MUTATION_FLAGS
        .iter()
        .copied()
        .enumerate()
        .find_map(|(raw, flags)| predicate(flags).then(|| state_id(raw)))
}

pub(crate) fn pattern_state_index(
    pattern: Pattern,
    cell: usize,
    cardinality: usize,
    rng: &mut u64,
) -> usize {
    if cardinality == 1 {
        return 0;
    }
    let x = cell & 15;
    let z = (cell >> 4) & 15;
    let y = (cell >> 8) & 15;
    match pattern {
        Pattern::Homogeneous => 0,
        Pattern::CardinalitySpread => cell % cardinality,
        Pattern::Layered => y % cardinality,
        Pattern::Clustered => ((x >> 2) + 4 * (z >> 2) + 16 * (y >> 2)) % cardinality,
        Pattern::Checker => (x + y + z) & 1,
        Pattern::Noisy => bounded(next_rng(rng), cardinality),
        Pattern::FluidContaining => fluid_state_index(cell, cardinality, rng),
        Pattern::SurvivalLike => survival_state_index(cell, cardinality, rng),
        Pattern::BuildLike => {
            let cluster = (x >> 1) + 8 * (z >> 1) + 64 * (y >> 1);
            let mixed = mix_u64(
                u64::try_from(cluster ^ cell.rotate_left(5)).expect("section index fits u64"),
            );
            bounded(mixed, cardinality)
        }
    }
}

fn survival_state_index(cell: usize, cardinality: usize, rng: &mut u64) -> usize {
    debug_assert!(cardinality >= 2);
    if cell.is_multiple_of(32) {
        0
    } else if cell.is_multiple_of(8) && cardinality > 2 {
        2 + bounded(next_rng(rng), cardinality - 2)
    } else {
        1
    }
}

fn fluid_state_index(cell: usize, cardinality: usize, rng: &mut u64) -> usize {
    debug_assert!(cardinality >= 3);
    if cell.is_multiple_of(16) {
        1
    } else if cell % 64 == 1 {
        0
    } else {
        2 + bounded(next_rng(rng), cardinality - 2)
    }
}

pub(crate) fn positive_contains_state<C>(prepared: &Prepared<C>) -> BlockStateId {
    *prepared
        .present_states
        .last()
        .expect("prepared section contains at least one state")
}

pub(crate) fn negative_contains_state<C>(prepared: &Prepared<C>) -> BlockStateId {
    (0..BLOCK_STATE_COUNT)
        .map(state_id)
        .find(|state| !prepared.present_states.contains(state))
        .expect("section cannot contain every target state")
}

pub(crate) fn make_positions(count: usize, seed: u64) -> Vec<SectionBlockPos> {
    let mut rng = BENCH_SEED ^ seed;
    (0..count)
        .map(|_| pos(bounded(next_rng(&mut rng), BLOCK_SECTION_CELLS)))
        .collect()
}

pub(crate) fn make_state_stream(
    states: &[BlockStateId],
    count: usize,
    seed: u64,
) -> Vec<BlockStateId> {
    assert!(!states.is_empty());
    let mut rng = BENCH_SEED ^ seed;
    (0..count)
        .map(|_| states[bounded(next_rng(&mut rng), states.len())])
        .collect()
}

pub(crate) fn state_id(index: usize) -> BlockStateId {
    let raw = u32::try_from(index).expect("benchmark state index fits u32");
    BlockStateId::new(raw).expect("benchmark state ID is inside target universe")
}

pub(crate) fn pos(cell: usize) -> SectionBlockPos {
    debug_assert!(cell < BLOCK_SECTION_CELLS);
    let x = u8::try_from(cell & 15).expect("bounded x");
    let z = u8::try_from((cell >> 4) & 15).expect("bounded z");
    let y = u8::try_from((cell >> 8) & 15).expect("bounded y");
    SectionBlockPos::new(x, y, z).expect("bounded section position")
}

pub(crate) fn next_rng(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

pub(crate) fn bounded(value: u64, bound: usize) -> usize {
    let bound = u64::try_from(bound).expect("benchmark bound fits u64");
    usize::try_from(value % bound).expect("bounded value fits usize")
}

fn mix_u64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crucible_generated::{AIR, GeneratedStateFacts, STATE_MUTATION_FLAGS};
    use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts};
    use crucible_world_section::DirectNBlockSection;

    use super::{
        COUNTED_FLUID, NON_AIR, CaseSpec, Pattern, pos, positive_contains_state, prepare,
    };

    fn observed_states(
        section: &DirectNBlockSection<crucible_generated::BlockStateId>,
    ) -> BTreeSet<crucible_generated::BlockStateId> {
        (0..BLOCK_SECTION_CELLS)
            .map(|cell| section.get(pos(cell)))
            .collect()
    }

    #[test]
    fn recorded_actual_cardinality_matches_final_semantic_image() {
        for case in [
            CaseSpec {
                pattern: Pattern::Layered,
                pool_cardinality: 8,
            },
            CaseSpec {
                pattern: Pattern::Noisy,
                pool_cardinality: 257,
            },
            CaseSpec {
                pattern: Pattern::BuildLike,
                pool_cardinality: 64,
            },
        ] {
            let prepared = prepare::<DirectNBlockSection<_>>(case);
            assert_eq!(prepared.actual_cardinality, observed_states(&prepared.section).len());
        }
    }

    #[test]
    fn positive_contains_needle_is_guaranteed_present() {
        let prepared = prepare::<DirectNBlockSection<_>>(CaseSpec {
            pattern: Pattern::Layered,
            pool_cardinality: 8,
        });
        let needle = positive_contains_state(&prepared);
        assert!(prepared.section.maybe_contains(|state| state == needle));
    }

    #[test]
    fn fluid_workload_contains_qualified_counted_fluid() {
        let prepared = prepare::<DirectNBlockSection<_>>(CaseSpec {
            pattern: Pattern::FluidContaining,
            pool_cardinality: 8,
        });
        let observed = observed_states(&prepared.section);
        assert!(observed.iter().any(|state| {
            GeneratedStateFacts.facts(*state).counted_fluid()
                && STATE_MUTATION_FLAGS[state.as_usize()] & COUNTED_FLUID != 0
        }));
    }

    #[test]
    fn survival_workload_is_air_plus_nonrandom_nonfluid_solids() {
        let prepared = prepare::<DirectNBlockSection<_>>(CaseSpec {
            pattern: Pattern::SurvivalLike,
            pool_cardinality: 32,
        });
        let mut air_cells = 0;
        let mut solid_cells = 0;
        for cell in 0..BLOCK_SECTION_CELLS {
            let state = prepared.section.get(pos(cell));
            if state == AIR {
                air_cells += 1;
            } else {
                assert_eq!(STATE_MUTATION_FLAGS[state.as_usize()], NON_AIR);
                solid_cells += 1;
            }
        }
        assert!(air_cells > 0);
        assert!(solid_cells > air_cells);
    }
}
