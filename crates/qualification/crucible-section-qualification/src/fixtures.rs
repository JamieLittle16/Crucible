//! Source-backed semantic fixtures for the frozen Minecraft 26.2 section contract.
//!
//! Fixtures record observations, not Mojang storage objects. Block fixtures name target semantic
//! fact signatures and expected section images; biome fixtures exercise the frozen lattice and
//! resolver order. The lowest qualified target state with a requested signature is selected
//! deterministically so the fixture remains independent of any live palette mechanism.

use std::fmt;

use crucible_generated::{
    AIR, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256, STATE_MUTATION_FLAGS,
};
use crucible_world_contract::{
    BIOME_SECTION_CELLS, BLOCK_SECTION_CELLS, BlockSection, SectionBiomePos, SectionBlockPos,
    SectionSummary,
};
use crucible_world_reference::{DirectBiomeSection, DirectBlockSection};
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

const MAGIC: &str = "CRUCIBLE-SECTION-SEMANTIC-FIXTURE";
const SCHEMA: u32 = 1;
const MINECRAFT_VERSION: &str = "26.2";
const PROTOCOL_VERSION: u32 = 776;
const DATA_VERSION: u32 = 4903;
const SOURCE_ARCHIVE_SHA256: &str =
    "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750";
const SOURCE_QUALIFICATION_SHA256: &str =
    "5d312d6025fa6556feaf5fa26c80577dcb024e7e5be5cd1bda98d101367600c8";
const RUNTIME_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Successful source-backed fixture qualification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixtureEvidence {
    cases: usize,
    block_candidate_checks: usize,
    biome_checks: usize,
    fingerprint: u64,
}

impl FixtureEvidence {
    /// Number of semantic cases in the fixture.
    #[must_use]
    pub const fn cases(self) -> usize {
        self.cases
    }

    /// Number of block-candidate executions performed.
    #[must_use]
    pub const fn block_candidate_checks(self) -> usize {
        self.block_candidate_checks
    }

    /// Number of biome cases performed.
    #[must_use]
    pub const fn biome_checks(self) -> usize {
        self.biome_checks
    }

    /// Stable FNV-1a fingerprint of the exact fixture bytes.
    #[must_use]
    pub const fn fingerprint(self) -> u64 {
        self.fingerprint
    }
}

/// Failure while parsing or qualifying a source-backed fixture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureFailure(String);

impl FixtureFailure {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for FixtureFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for FixtureFailure {}

#[derive(Clone, Debug)]
struct StateBinding {
    label: String,
    state: BlockStateId,
}

#[derive(Clone, Debug)]
enum Case {
    BlockFill {
        name: String,
        state: String,
        expected: SectionSummary,
    },
    BlockOne {
        name: String,
        state: String,
        cell: u16,
        expected: SectionSummary,
    },
    BlockReverse {
        name: String,
        state: String,
        cell: u16,
    },
    BiomeFillOrder,
    BiomeReplace {
        x: u8,
        y: u8,
        z: u8,
        before: u16,
        after: u16,
    },
}

/// Parses and qualifies one target-version semantic fixture document.
///
/// Block cases are checked against the permanent direct oracle and all four admitted live block
/// candidates. Biome cases are checked against the direct 64-cell reference lattice because M0.3B
/// has not admitted an optimized biome-storage mechanism.
///
/// # Errors
///
/// Returns [`FixtureFailure`] for malformed input, target/provenance drift, an unavailable target
/// fact signature, or any semantic disagreement.
pub fn qualify_fixture(input: &str) -> Result<FixtureEvidence, FixtureFailure> {
    let mut lines = input.lines();
    validate_header(
        lines
            .next()
            .ok_or_else(|| FixtureFailure::new("empty fixture"))?,
    )?;
    validate_provenance(
        lines
            .next()
            .ok_or_else(|| FixtureFailure::new("missing fixture provenance"))?,
    )?;

    let mut states = Vec::new();
    let mut cases = Vec::new();
    for (offset, line) in lines.enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        parse_line(line, offset + 3, &mut states, &mut cases)?;
    }
    if states.is_empty() || cases.is_empty() {
        return Err(FixtureFailure::new("fixture must define states and cases"));
    }

    let mut block_candidate_checks = 0;
    let mut biome_checks = 0;
    for case in &cases {
        match case {
            Case::BlockFill { .. } | Case::BlockOne { .. } | Case::BlockReverse { .. } => {
                qualify_block::<DirectNBlockSection<BlockStateId>, _>(case, &states, |state| {
                    DirectNBlockSection::filled(state, &GeneratedStateFacts)
                })?;
                qualify_block::<AdaptiveBlockSection<BlockStateId>, _>(case, &states, |state| {
                    AdaptiveBlockSection::filled(state, &GeneratedStateFacts)
                })?;
                qualify_block::<FastLocalBlockSection<BlockStateId>, _>(case, &states, |state| {
                    FastLocalBlockSection::filled(state, &GeneratedStateFacts)
                })?;
                qualify_block::<PackedLocalBlockSection<BlockStateId>, _>(case, &states, |state| {
                    PackedLocalBlockSection::filled(state, &GeneratedStateFacts)
                })?;
                block_candidate_checks += 4;
            }
            Case::BiomeFillOrder => {
                qualify_biome_fill_order()?;
                biome_checks += 1;
            }
            Case::BiomeReplace {
                x,
                y,
                z,
                before,
                after,
            } => {
                qualify_biome_replace(*x, *y, *z, *before, *after)?;
                biome_checks += 1;
            }
        }
    }

    Ok(FixtureEvidence {
        cases: cases.len(),
        block_candidate_checks,
        biome_checks,
        fingerprint: fnv1a64(input.as_bytes()),
    })
}

fn validate_header(line: &str) -> Result<(), FixtureFailure> {
    let parts = line.split('|').collect::<Vec<_>>();
    if parts.len() != 5 || parts[0] != MAGIC {
        return Err(FixtureFailure::new("invalid fixture header"));
    }
    if parse_u32(parts[1], "fixture schema")? != SCHEMA
        || parts[2] != MINECRAFT_VERSION
        || parse_u32(parts[3], "protocol version")? != PROTOCOL_VERSION
        || parse_u32(parts[4], "data version")? != DATA_VERSION
    {
        return Err(FixtureFailure::new(
            "fixture target pin differs from build target",
        ));
    }
    Ok(())
}

fn validate_provenance(line: &str) -> Result<(), FixtureFailure> {
    let expected = [
        "PROVENANCE",
        SOURCE_ARCHIVE_SHA256,
        SOURCE_QUALIFICATION_SHA256,
        RUNTIME_SERVER_SHA256,
        STATE_DATA_GENERATION_SHA256,
    ];
    if line.split('|').collect::<Vec<_>>() != expected {
        return Err(FixtureFailure::new(
            "fixture provenance differs from qualified 26.2 evidence",
        ));
    }
    Ok(())
}

fn parse_line(
    line: &str,
    line_number: usize,
    states: &mut Vec<StateBinding>,
    cases: &mut Vec<Case>,
) -> Result<(), FixtureFailure> {
    let parts = line.split('|').collect::<Vec<_>>();
    match parts.as_slice() {
        ["STATE", label, flags] => {
            if states.iter().any(|state| state.label == *label) {
                return Err(FixtureFailure::new(format!(
                    "duplicate state label at line {line_number}"
                )));
            }
            let flags = parse_u8(flags, "state flags")?;
            let state = find_target_state(flags).ok_or_else(|| {
                FixtureFailure::new(format!(
                    "no target state has flags {flags} at line {line_number}"
                ))
            })?;
            states.push(StateBinding {
                label: (*label).to_owned(),
                state,
            });
        }
        [
            "BLOCK-FILL",
            name,
            state,
            non_air,
            fluid,
            block_tick,
            fluid_tick,
        ] => {
            cases.push(Case::BlockFill {
                name: (*name).to_owned(),
                state: (*state).to_owned(),
                expected: parse_summary(non_air, fluid, block_tick, fluid_tick)?,
            });
        }
        [
            "BLOCK-ONE",
            name,
            state,
            cell,
            non_air,
            fluid,
            block_tick,
            fluid_tick,
        ] => {
            cases.push(Case::BlockOne {
                name: (*name).to_owned(),
                state: (*state).to_owned(),
                cell: parse_cell(cell)?,
                expected: parse_summary(non_air, fluid, block_tick, fluid_tick)?,
            });
        }
        ["BLOCK-REVERSE", name, state, cell] => {
            cases.push(Case::BlockReverse {
                name: (*name).to_owned(),
                state: (*state).to_owned(),
                cell: parse_cell(cell)?,
            });
        }
        ["BIOME-FILL-ORDER", "x-major-y-z"] => cases.push(Case::BiomeFillOrder),
        ["BIOME-REPLACE", x, y, z, before, after] => cases.push(Case::BiomeReplace {
            x: parse_coord(x)?,
            y: parse_coord(y)?,
            z: parse_coord(z)?,
            before: parse_u16(before, "biome before")?,
            after: parse_u16(after, "biome after")?,
        }),
        _ => {
            return Err(FixtureFailure::new(format!(
                "invalid semantic fixture at line {line_number}"
            )));
        }
    }
    Ok(())
}

fn qualify_block<C, F>(case: &Case, states: &[StateBinding], build: F) -> Result<(), FixtureFailure>
where
    C: BlockSection<BlockStateId>,
    F: Fn(BlockStateId) -> C,
{
    match case {
        Case::BlockFill {
            name,
            state,
            expected,
        } => {
            let target = resolve_state(states, state)?;
            let candidate = build(target);
            let reference = DirectBlockSection::filled(target, &GeneratedStateFacts);
            compare_image(name, &candidate, &reference, *expected)
        }
        Case::BlockOne {
            name,
            state,
            cell,
            expected,
        } => {
            let target = resolve_state(states, state)?;
            let mut candidate = build(AIR);
            let mut reference = DirectBlockSection::filled(AIR, &GeneratedStateFacts);
            let position = block_pos(*cell);
            if candidate.replace(position, target, &GeneratedStateFacts) != AIR
                || reference.replace(position, target, &GeneratedStateFacts) != AIR
            {
                return Err(FixtureFailure::new(format!("{name}: wrong previous state")));
            }
            compare_image(name, &candidate, &reference, *expected)
        }
        Case::BlockReverse { name, state, cell } => {
            let target = resolve_state(states, state)?;
            let mut candidate = build(AIR);
            let mut reference = DirectBlockSection::filled(AIR, &GeneratedStateFacts);
            let position = block_pos(*cell);
            candidate.replace(position, target, &GeneratedStateFacts);
            reference.replace(position, target, &GeneratedStateFacts);
            if candidate.replace(position, AIR, &GeneratedStateFacts) != target
                || reference.replace(position, AIR, &GeneratedStateFacts) != target
            {
                return Err(FixtureFailure::new(format!(
                    "{name}: reversal returned wrong previous state"
                )));
            }
            compare_image(name, &candidate, &reference, SectionSummary::default())
        }
        Case::BiomeFillOrder | Case::BiomeReplace { .. } => {
            Err(FixtureFailure::new("block qualifier received biome case"))
        }
    }
}

fn compare_image<C: BlockSection<BlockStateId>>(
    name: &str,
    candidate: &C,
    reference: &DirectBlockSection<BlockStateId>,
    expected: SectionSummary,
) -> Result<(), FixtureFailure> {
    if reference.summary() != expected
        || reference.recompute_summary(&GeneratedStateFacts) != expected
        || candidate.summary() != expected
    {
        return Err(FixtureFailure::new(format!(
            "{name}: summary differs from source-backed fixture"
        )));
    }
    for cell in 0..BLOCK_SECTION_CELLS {
        let position = block_pos(u16::try_from(cell).expect("4096 cells fit u16"));
        if candidate.get(position) != reference.get(position) {
            return Err(FixtureFailure::new(format!(
                "{name}: semantic cell differs at {cell}"
            )));
        }
    }
    Ok(())
}

fn qualify_biome_fill_order() -> Result<(), FixtureFailure> {
    let mut section = DirectBiomeSection::filled(u16::MAX);
    let mut observed = Vec::with_capacity(BIOME_SECTION_CELLS);
    let mut ordinal = 0_u16;
    section.fill_with(|x, y, z| {
        observed.push((x, y, z));
        let result = ordinal;
        ordinal += 1;
        result
    });

    let mut expected_order = Vec::with_capacity(BIOME_SECTION_CELLS);
    let mut expected_value = 0_u16;
    for x in 0..4 {
        for y in 0..4 {
            for z in 0..4 {
                expected_order.push((x, y, z));
                let position = SectionBiomePos::new(x, y, z).expect("bounded biome coordinate");
                if section.get(position) != expected_value {
                    return Err(FixtureFailure::new(
                        "biome resolver value stored at wrong semantic coordinate",
                    ));
                }
                expected_value += 1;
            }
        }
    }
    if observed != expected_order {
        return Err(FixtureFailure::new(
            "biome resolver call order differs from x-major/y/z target order",
        ));
    }
    Ok(())
}

fn qualify_biome_replace(
    x: u8,
    y: u8,
    z: u8,
    before: u16,
    after: u16,
) -> Result<(), FixtureFailure> {
    let position = SectionBiomePos::new(x, y, z)
        .ok_or_else(|| FixtureFailure::new("invalid biome coordinate"))?;
    let mut section = DirectBiomeSection::filled(before);
    if section.replace(position, after) != before || section.get(position) != after {
        return Err(FixtureFailure::new("biome replacement semantics differ"));
    }
    Ok(())
}

fn resolve_state(states: &[StateBinding], label: &str) -> Result<BlockStateId, FixtureFailure> {
    states
        .iter()
        .find(|state| state.label == label)
        .map(|state| state.state)
        .ok_or_else(|| FixtureFailure::new(format!("unknown state label: {label}")))
}

fn find_target_state(flags: u8) -> Option<BlockStateId> {
    STATE_MUTATION_FLAGS
        .iter()
        .position(|value| *value == flags)
        .and_then(|index| u32::try_from(index).ok())
        .and_then(BlockStateId::new)
}

fn parse_summary(
    non_air: &str,
    fluid: &str,
    block_tick: &str,
    fluid_tick: &str,
) -> Result<SectionSummary, FixtureFailure> {
    Ok(SectionSummary {
        non_air_count: parse_u16(non_air, "non-air count")?,
        fluid_count: parse_u16(fluid, "fluid count")?,
        random_block_present: parse_bool(block_tick, "random-block presence")?,
        random_fluid_present: parse_bool(fluid_tick, "random-fluid presence")?,
    })
}

fn parse_cell(value: &str) -> Result<u16, FixtureFailure> {
    let cell = parse_u16(value, "block cell")?;
    if usize::from(cell) >= BLOCK_SECTION_CELLS {
        return Err(FixtureFailure::new("block cell outside 4096-cell domain"));
    }
    Ok(cell)
}

fn parse_coord(value: &str) -> Result<u8, FixtureFailure> {
    let coordinate = parse_u8(value, "biome coordinate")?;
    if coordinate >= 4 {
        return Err(FixtureFailure::new("biome coordinate outside 4-cell axis"));
    }
    Ok(coordinate)
}

fn parse_bool(value: &str, label: &str) -> Result<bool, FixtureFailure> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(FixtureFailure::new(format!("invalid {label}"))),
    }
}

fn parse_u8(value: &str, label: &str) -> Result<u8, FixtureFailure> {
    value
        .parse::<u8>()
        .map_err(|_| FixtureFailure::new(format!("invalid {label}")))
}

fn parse_u16(value: &str, label: &str) -> Result<u16, FixtureFailure> {
    value
        .parse::<u16>()
        .map_err(|_| FixtureFailure::new(format!("invalid {label}")))
}

fn parse_u32(value: &str, label: &str) -> Result<u32, FixtureFailure> {
    value
        .parse::<u32>()
        .map_err(|_| FixtureFailure::new(format!("invalid {label}")))
}

fn block_pos(cell: u16) -> SectionBlockPos {
    let cell = usize::from(cell);
    let x = u8::try_from(cell & 15).expect("x nibble fits u8");
    let z = u8::try_from((cell >> 4) & 15).expect("z nibble fits u8");
    let y = u8::try_from((cell >> 8) & 15).expect("y nibble fits u8");
    SectionBlockPos::new(x, y, z).expect("decoded block cell is bounded")
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}
