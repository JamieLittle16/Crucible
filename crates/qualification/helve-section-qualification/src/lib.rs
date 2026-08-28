//! Deterministic semantic qualification for live block-section candidates.
//!
//! This crate is deliberately mechanism-independent. It owns versioned operation traces, runs the
//! same traces against the permanent direct oracle and each optimized candidate, verifies target
//! generated facts, and emits stable equivalence evidence for M0.3C.

#![forbid(unsafe_code)]

use std::fmt::{self, Write as _};

use helve_generated::{
    AIR, BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256,
    STATE_DATA_INPUT_SHA256, STATE_MUTATION_FLAGS,
};
use helve_world_contract::{
    BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionStateFacts,
};
use helve_world_reference::DirectBlockSection;
use helve_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

/// Version of the serialized deterministic section trace format.
pub const TRACE_SCHEMA: u32 = 1;
/// Version of the emitted section equivalence evidence format.
pub const EVIDENCE_SCHEMA: u32 = 1;
/// Target Minecraft version qualified by this M0 harness.
pub const MINECRAFT_VERSION: &str = "26.2";
/// Target protocol version qualified by this M0 harness.
pub const PROTOCOL_VERSION: u32 = 776;
/// Target data/world version qualified by this M0 harness.
pub const DATA_VERSION: u32 = 4903;

const TRACE_MAGIC: &str = "CRUCIBLE-SECTION-TRACE";
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

const SEM_IDS: [&str; 12] = [
    "SEM-WORLD-SECTION-001",
    "SEM-WORLD-SECTION-002",
    "SEM-WORLD-SECTION-005",
    "SEM-WORLD-SECTION-006",
    "SEM-WORLD-SECTION-007",
    "SEM-WORLD-SECTION-008",
    "SEM-WORLD-SECTION-009",
    "SEM-WORLD-SECTION-010",
    "SEM-WORLD-SECTION-011",
    "SEM-WORLD-SECTION-012",
    "SEM-WORLD-SECTION-013",
    "SEM-WORLD-SECTION-014",
];

/// Qualification tier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualificationMode {
    /// Bounded deterministic suite suitable for pull-request CI.
    Quick,
    /// Multi-seed extended suite with millions of target mutations.
    Full,
}

impl QualificationMode {
    /// Returns the stable evidence spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Full => "full",
        }
    }
}

/// Live CPU candidate under qualification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Candidate {
    /// Direct 4096-state production-mechanism baseline.
    Direct,
    /// `Uniform -> Local4Stable -> Local8Stable -> DirectN`.
    Adaptive,
    /// `Uniform -> Local8Stable -> DirectN`.
    FastLocal,
    /// `Uniform -> packed 1..8-bit local -> DirectN`.
    PackedLocal,
}

impl Candidate {
    /// All currently admitted M0.3B candidates.
    pub const ALL: [Self; 4] = [
        Self::Direct,
        Self::Adaptive,
        Self::FastLocal,
        Self::PackedLocal,
    ];

    /// Returns the stable CLI/evidence name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Adaptive => "adaptive",
            Self::FastLocal => "fast-local",
            Self::PackedLocal => "packed-local",
        }
    }

    /// Resolves a stable CLI/evidence name.
    #[must_use]
    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "direct" => Some(Self::Direct),
            "adaptive" => Some(Self::Adaptive),
            "fast-local" => Some(Self::FastLocal),
            "packed-local" => Some(Self::PackedLocal),
            _ => None,
        }
    }
}

/// Deterministic trace family.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceClass {
    AllAir,
    OneCellReversal,
    LocalizedChurn,
    RandomUniformWrites,
    HighEntropyWrites,
    DeadPaletteChurn,
    Boundary16,
    Boundary256,
    LongSeeded,
}

impl TraceClass {
    const fn as_str(self) -> &'static str {
        match self {
            Self::AllAir => "all-air",
            Self::OneCellReversal => "one-cell-reversal",
            Self::LocalizedChurn => "localized-churn",
            Self::RandomUniformWrites => "random-uniform-writes",
            Self::HighEntropyWrites => "high-entropy-writes",
            Self::DeadPaletteChurn => "dead-palette-churn",
            Self::Boundary16 => "boundary-16-17",
            Self::Boundary256 => "boundary-256-257",
            Self::LongSeeded => "long-seeded",
        }
    }

    fn from_name(value: &str) -> Option<Self> {
        match value {
            "all-air" => Some(Self::AllAir),
            "one-cell-reversal" => Some(Self::OneCellReversal),
            "localized-churn" => Some(Self::LocalizedChurn),
            "random-uniform-writes" => Some(Self::RandomUniformWrites),
            "high-entropy-writes" => Some(Self::HighEntropyWrites),
            "dead-palette-churn" => Some(Self::DeadPaletteChurn),
            "boundary-16-17" => Some(Self::Boundary16),
            "boundary-256-257" => Some(Self::Boundary256),
            "long-seeded" => Some(Self::LongSeeded),
            _ => None,
        }
    }
}

/// One serialized semantic operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TraceOp {
    Get(u16),
    Replace { cell: u16, state: u16 },
    ReplaceSame(u16),
    Summary,
    Contains(u16),
    Checkpoint,
}

/// Versioned deterministic operation stream.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trace {
    class: TraceClass,
    seed: u64,
    operations: Vec<TraceOp>,
}

impl Trace {
    fn new(class: TraceClass, seed: u64) -> Self {
        Self {
            class,
            seed,
            operations: Vec::new(),
        }
    }

    /// Returns the trace class.
    #[must_use]
    pub const fn class(&self) -> TraceClass {
        self.class
    }

    /// Returns the deterministic seed.
    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// Returns the serialized semantic operations.
    #[must_use]
    pub fn operations(&self) -> &[TraceOp] {
        &self.operations
    }

    /// Encodes this trace in the stable line-oriented v1 format.
    #[must_use]
    pub fn encode(&self) -> String {
        let mut output = String::new();
        let _ = writeln!(
            output,
            "{TRACE_MAGIC}|{TRACE_SCHEMA}|{}|{:016x}",
            self.class.as_str(),
            self.seed
        );

        for operation in &self.operations {
            let _ = match operation {
                TraceOp::Get(cell) => writeln!(output, "G|{cell}"),
                TraceOp::Replace { cell, state } => writeln!(output, "R|{cell}|{state}"),
                TraceOp::ReplaceSame(cell) => writeln!(output, "N|{cell}"),
                TraceOp::Summary => writeln!(output, "S"),
                TraceOp::Contains(state) => writeln!(output, "C|{state}"),
                TraceOp::Checkpoint => writeln!(output, "K"),
            };
        }
        output
    }

    /// Decodes a v1 trace and rejects malformed or out-of-domain operations.
    ///
    /// # Errors
    ///
    /// Returns [`QualificationFailure`] for a bad header/schema, unknown trace class, malformed
    /// operation, or any cell/state identity outside the frozen target domains.
    pub fn decode(input: &str) -> Result<Self, QualificationFailure> {
        let mut lines = input.lines();
        let header = lines
            .next()
            .ok_or_else(|| QualificationFailure::new("trace is empty"))?;
        let header_parts = header.split('|').collect::<Vec<_>>();
        if header_parts.len() != 4 || header_parts[0] != TRACE_MAGIC {
            return Err(QualificationFailure::new("invalid trace header"));
        }
        let schema = header_parts[1]
            .parse::<u32>()
            .map_err(|_| QualificationFailure::new("invalid trace schema"))?;
        if schema != TRACE_SCHEMA {
            return Err(QualificationFailure::new("unsupported trace schema"));
        }
        let class = TraceClass::from_name(header_parts[2])
            .ok_or_else(|| QualificationFailure::new("unknown trace class"))?;
        let seed = u64::from_str_radix(header_parts[3], 16)
            .map_err(|_| QualificationFailure::new("invalid trace seed"))?;
        let mut trace = Self::new(class, seed);

        for (offset, line) in lines.enumerate() {
            let line_number = offset + 2;
            let parts = line.split('|').collect::<Vec<_>>();
            let operation = match parts.as_slice() {
                ["G", cell] => TraceOp::Get(parse_cell(cell, line_number)?),
                ["R", cell, state] => TraceOp::Replace {
                    cell: parse_cell(cell, line_number)?,
                    state: parse_state(state, line_number)?,
                },
                ["N", cell] => TraceOp::ReplaceSame(parse_cell(cell, line_number)?),
                ["S"] => TraceOp::Summary,
                ["C", state] => TraceOp::Contains(parse_state(state, line_number)?),
                ["K"] => TraceOp::Checkpoint,
                _ => {
                    return Err(QualificationFailure::new(format!(
                        "invalid trace operation at line {line_number}"
                    )));
                }
            };
            trace.operations.push(operation);
        }
        Ok(trace)
    }
}

/// One candidate's equivalence evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRecord {
    candidate: Candidate,
    mode: QualificationMode,
    trace_count: usize,
    trace_operations: usize,
    synthetic_operations: usize,
    trace_fingerprint: u64,
}

impl EvidenceRecord {
    /// Stable EQUIV identifier.
    #[must_use]
    pub fn id(&self) -> String {
        format!(
            "EQUIV-WORLD-SECTION-{}-{}",
            self.mode.as_str().to_ascii_uppercase(),
            self.candidate
                .as_str()
                .to_ascii_uppercase()
                .replace('-', "_")
        )
    }

    /// Candidate represented by this record.
    #[must_use]
    pub const fn candidate(&self) -> Candidate {
        self.candidate
    }

    /// Number of deterministic target trace operations executed.
    #[must_use]
    pub const fn trace_operations(&self) -> usize {
        self.trace_operations
    }
}

/// Successful section qualification report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualificationReport {
    mode: QualificationMode,
    records: Vec<EvidenceRecord>,
}

impl QualificationReport {
    /// Evidence records, one per qualified candidate.
    #[must_use]
    pub fn records(&self) -> &[EvidenceRecord] {
        &self.records
    }

    /// Serializes stable evidence JSON without introducing a serialization dependency.
    #[must_use]
    pub fn to_json(&self, commit_sha: &str) -> String {
        let mut output = String::new();
        let _ = writeln!(output, "{{");
        let _ = writeln!(output, "  \"schema\": {EVIDENCE_SCHEMA},");
        let _ = writeln!(output, "  \"qualification\": \"section\",");
        let _ = writeln!(output, "  \"mode\": \"{}\",", self.mode.as_str());
        let _ = writeln!(output, "  \"minecraft_version\": \"{MINECRAFT_VERSION}\",");
        let _ = writeln!(output, "  \"protocol_version\": {PROTOCOL_VERSION},");
        let _ = writeln!(output, "  \"data_version\": {DATA_VERSION},");
        let _ = writeln!(
            output,
            "  \"commit_sha\": \"{}\",",
            json_safe_token(commit_sha)
        );
        let _ = writeln!(output, "  \"state_count\": {BLOCK_STATE_COUNT},");
        let _ = writeln!(
            output,
            "  \"state_data_input_sha256\": \"{STATE_DATA_INPUT_SHA256}\","
        );
        let _ = writeln!(
            output,
            "  \"state_data_generation_sha256\": \"{STATE_DATA_GENERATION_SHA256}\","
        );
        let _ = writeln!(output, "  \"trace_schema\": {TRACE_SCHEMA},");
        let _ = writeln!(output, "  \"sem_ids\": [");
        for (index, sem_id) in SEM_IDS.iter().enumerate() {
            let suffix = if index + 1 == SEM_IDS.len() { "" } else { "," };
            let _ = writeln!(output, "    \"{sem_id}\"{suffix}");
        }
        let _ = writeln!(output, "  ],");
        let _ = writeln!(output, "  \"records\": [");
        for (index, record) in self.records.iter().enumerate() {
            let suffix = if index + 1 == self.records.len() {
                ""
            } else {
                ","
            };
            let _ = writeln!(output, "    {{");
            let _ = writeln!(output, "      \"id\": \"{}\",", record.id());
            let _ = writeln!(
                output,
                "      \"candidate\": \"{}\",",
                record.candidate.as_str()
            );
            let _ = writeln!(output, "      \"trace_count\": {},", record.trace_count);
            let _ = writeln!(
                output,
                "      \"trace_operations\": {},",
                record.trace_operations
            );
            let _ = writeln!(
                output,
                "      \"synthetic_operations\": {},",
                record.synthetic_operations
            );
            let _ = writeln!(
                output,
                "      \"trace_fingerprint_fnv1a64\": \"{:016x}\"",
                record.trace_fingerprint
            );
            let _ = writeln!(output, "    }}{suffix}");
        }
        let _ = writeln!(output, "  ]");
        let _ = writeln!(output, "}}");
        output
    }
}

/// Deterministic qualification failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualificationFailure {
    detail: String,
}

impl QualificationFailure {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for QualificationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for QualificationFailure {}

trait CandidateFactory<S>: BlockSection<S> + Clone + Sized
where
    S: Copy + Eq,
{
    fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self;
}

impl<S: Copy + Eq> CandidateFactory<S> for DirectNBlockSection<S> {
    fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        DirectNBlockSection::filled(state, facts)
    }
}

impl<S: Copy + Eq> CandidateFactory<S> for AdaptiveBlockSection<S> {
    fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        AdaptiveBlockSection::filled(state, facts)
    }
}

impl<S: Copy + Eq> CandidateFactory<S> for FastLocalBlockSection<S> {
    fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        FastLocalBlockSection::filled(state, facts)
    }
}

impl<S: Copy + Eq> CandidateFactory<S> for PackedLocalBlockSection<S> {
    fn filled<F: BlockStateFacts<S>>(state: S, facts: &F) -> Self {
        PackedLocalBlockSection::filled(state, facts)
    }
}

/// Runs the selected deterministic qualification tier.
///
/// # Errors
///
/// Returns [`QualificationFailure`] when generated target facts disagree with their committed
/// table, trace serialization is unstable, the permanent reference violates recomputation, or any
/// selected candidate diverges from the reference on an observed semantic obligation.
pub fn qualify(
    mode: QualificationMode,
    candidate_filter: Option<Candidate>,
) -> Result<QualificationReport, QualificationFailure> {
    validate_generated_facts()?;
    let traces = traces_for(mode);
    for trace in &traces {
        let encoded = trace.encode();
        let decoded = Trace::decode(&encoded)?;
        if decoded != *trace {
            return Err(QualificationFailure::new(format!(
                "trace round-trip changed {} seed {:016x}",
                trace.class.as_str(),
                trace.seed
            )));
        }
    }

    let candidates =
        candidate_filter.map_or_else(|| Candidate::ALL.to_vec(), |candidate| vec![candidate]);
    let fingerprint = trace_fingerprint(&traces);
    let trace_operations = traces
        .iter()
        .map(|trace| trace.operations.len())
        .sum::<usize>();
    let mut records = Vec::with_capacity(candidates.len());

    for candidate in candidates {
        let synthetic_operations = match candidate {
            Candidate::Direct => {
                qualify_candidate::<DirectNBlockSection<BlockStateId>>(&traces, candidate)?;
                qualify_synthetic::<DirectNBlockSection<u8>>(candidate)?
            }
            Candidate::Adaptive => {
                qualify_candidate::<AdaptiveBlockSection<BlockStateId>>(&traces, candidate)?;
                qualify_synthetic::<AdaptiveBlockSection<u8>>(candidate)?
            }
            Candidate::FastLocal => {
                qualify_candidate::<FastLocalBlockSection<BlockStateId>>(&traces, candidate)?;
                qualify_synthetic::<FastLocalBlockSection<u8>>(candidate)?
            }
            Candidate::PackedLocal => {
                qualify_candidate::<PackedLocalBlockSection<BlockStateId>>(&traces, candidate)?;
                qualify_synthetic::<PackedLocalBlockSection<u8>>(candidate)?
            }
        };
        records.push(EvidenceRecord {
            candidate,
            mode,
            trace_count: traces.len(),
            trace_operations,
            synthetic_operations,
            trace_fingerprint: fingerprint,
        });
    }

    Ok(QualificationReport { mode, records })
}

fn qualify_candidate<C>(
    traces: &[Trace],
    candidate_name: Candidate,
) -> Result<(), QualificationFailure>
where
    C: CandidateFactory<BlockStateId>,
{
    for trace in traces {
        let mut candidate = C::filled(AIR, &GeneratedStateFacts);
        let mut reference = DirectBlockSection::filled(AIR, &GeneratedStateFacts);
        for (operation_index, operation) in trace.operations.iter().enumerate() {
            run_operation(
                &mut candidate,
                &mut reference,
                *operation,
                candidate_name,
                trace,
                operation_index,
            )?;
        }
        checkpoint(
            &candidate,
            &reference,
            candidate_name,
            trace,
            trace.operations.len(),
        )?;
    }
    check_clone_independence::<C>(candidate_name)
}

fn run_operation<C: BlockSection<BlockStateId>>(
    candidate: &mut C,
    reference: &mut DirectBlockSection<BlockStateId>,
    operation: TraceOp,
    candidate_name: Candidate,
    trace: &Trace,
    operation_index: usize,
) -> Result<(), QualificationFailure> {
    match operation {
        TraceOp::Get(cell) => {
            let position = pos(cell);
            if candidate.get(position) != reference.get(position) {
                return trace_failure(candidate_name, trace, operation_index, "get mismatch");
            }
        }
        TraceOp::Replace { cell, state } => {
            let position = pos(cell);
            let state = state_id(state);
            let candidate_previous = candidate.replace(position, state, &GeneratedStateFacts);
            let reference_previous = reference.replace(position, state, &GeneratedStateFacts);
            if candidate_previous != reference_previous {
                return trace_failure(
                    candidate_name,
                    trace,
                    operation_index,
                    "previous-state mismatch",
                );
            }
            if candidate.get(position) != state {
                return trace_failure(
                    candidate_name,
                    trace,
                    operation_index,
                    "replacement did not install requested state",
                );
            }
            compare_incremental_summaries(
                candidate,
                reference,
                candidate_name,
                trace,
                operation_index,
            )?;
        }
        TraceOp::ReplaceSame(cell) => {
            let position = pos(cell);
            let state = reference.get(position);
            let before = candidate.summary();
            let candidate_previous = candidate.replace(position, state, &GeneratedStateFacts);
            let reference_previous = reference.replace(position, state, &GeneratedStateFacts);
            if candidate_previous != reference_previous || candidate_previous != state {
                return trace_failure(
                    candidate_name,
                    trace,
                    operation_index,
                    "same-state previous-state mismatch",
                );
            }
            if candidate.summary() != before {
                return trace_failure(
                    candidate_name,
                    trace,
                    operation_index,
                    "same-state replacement changed summary",
                );
            }
            compare_incremental_summaries(
                candidate,
                reference,
                candidate_name,
                trace,
                operation_index,
            )?;
        }
        TraceOp::Summary => {
            compare_summaries(candidate, reference, candidate_name, trace, operation_index)?;
        }
        TraceOp::Contains(state) => {
            let state = state_id(state);
            let exact =
                (0..BLOCK_SECTION_CELLS).any(|cell| reference.get(pos_from_usize(cell)) == state);
            if !candidate.maybe_contains(|value| value == state) && exact {
                return trace_failure(
                    candidate_name,
                    trace,
                    operation_index,
                    "maybe_contains false negative",
                );
            }
        }
        TraceOp::Checkpoint => {
            checkpoint(candidate, reference, candidate_name, trace, operation_index)?;
        }
    }
    Ok(())
}

fn checkpoint<C: BlockSection<BlockStateId>>(
    candidate: &C,
    reference: &DirectBlockSection<BlockStateId>,
    candidate_name: Candidate,
    trace: &Trace,
    operation_index: usize,
) -> Result<(), QualificationFailure> {
    for cell in 0..BLOCK_SECTION_CELLS {
        let position = pos_from_usize(cell);
        if candidate.get(position) != reference.get(position) {
            return trace_failure(
                candidate_name,
                trace,
                operation_index,
                "checkpoint cell mismatch",
            );
        }
    }
    compare_summaries(candidate, reference, candidate_name, trace, operation_index)
}

fn compare_incremental_summaries<C: BlockSection<BlockStateId>>(
    candidate: &C,
    reference: &DirectBlockSection<BlockStateId>,
    candidate_name: Candidate,
    trace: &Trace,
    operation_index: usize,
) -> Result<(), QualificationFailure> {
    if candidate.summary() != reference.summary() {
        return trace_failure(
            candidate_name,
            trace,
            operation_index,
            "candidate/reference incremental summary mismatch",
        );
    }
    Ok(())
}

fn compare_summaries<C: BlockSection<BlockStateId>>(
    candidate: &C,
    reference: &DirectBlockSection<BlockStateId>,
    candidate_name: Candidate,
    trace: &Trace,
    operation_index: usize,
) -> Result<(), QualificationFailure> {
    let recomputed = reference.recompute_summary(&GeneratedStateFacts);
    if reference.summary() != recomputed {
        return trace_failure(
            candidate_name,
            trace,
            operation_index,
            "reference incremental/recompute mismatch",
        );
    }
    if candidate.summary() != recomputed {
        return trace_failure(
            candidate_name,
            trace,
            operation_index,
            "candidate summary/recompute mismatch",
        );
    }
    Ok(())
}

fn check_clone_independence<C>(candidate_name: Candidate) -> Result<(), QualificationFailure>
where
    C: CandidateFactory<BlockStateId>,
{
    let mut original = C::filled(AIR, &GeneratedStateFacts);
    let first = pos(17);
    let second = pos(31);
    original.replace(first, state_id(1), &GeneratedStateFacts);
    let original_summary = original.summary();
    let mut cloned = original.clone();
    cloned.replace(first, state_id(2), &GeneratedStateFacts);
    if original.get(first) != state_id(1) || original.summary() != original_summary {
        return Err(QualificationFailure::new(format!(
            "{} clone mutation altered original",
            candidate_name.as_str()
        )));
    }
    original.replace(second, state_id(3), &GeneratedStateFacts);
    if cloned.get(second) != AIR {
        return Err(QualificationFailure::new(format!(
            "{} original mutation altered clone",
            candidate_name.as_str()
        )));
    }
    Ok(())
}

struct SyntheticFacts;

impl BlockStateFacts<u8> for SyntheticFacts {
    fn facts(&self, state: u8) -> SectionStateFacts {
        SectionStateFacts::new(
            state & 0b0001 != 0,
            state & 0b0010 != 0,
            state & 0b0100 != 0,
            state & 0b1000 != 0,
        )
    }
}

fn qualify_synthetic<C>(candidate_name: Candidate) -> Result<usize, QualificationFailure>
where
    C: CandidateFactory<u8>,
{
    let mut candidate = C::filled(0, &SyntheticFacts);
    let mut reference = DirectBlockSection::filled(0, &SyntheticFacts);
    let mut operations = 0_usize;

    for raw in 0_u8..16 {
        let position = pos(u16::from(raw));
        let candidate_previous = candidate.replace(position, raw, &SyntheticFacts);
        let reference_previous = reference.replace(position, raw, &SyntheticFacts);
        operations += 1;
        if candidate_previous != reference_previous || candidate.summary() != reference.summary() {
            return Err(QualificationFailure::new(format!(
                "{} synthetic flag-combination mismatch at raw {raw}",
                candidate_name.as_str()
            )));
        }
    }

    for step in 0..4096_usize {
        let cell = pos_from_usize(step);
        let raw = u8::try_from((step * 7) & 15).expect("masked to four bits");
        let candidate_previous = candidate.replace(cell, raw, &SyntheticFacts);
        let reference_previous = reference.replace(cell, raw, &SyntheticFacts);
        operations += 1;
        if candidate_previous != reference_previous || candidate.summary() != reference.summary() {
            return Err(QualificationFailure::new(format!(
                "{} synthetic churn mismatch at step {step}",
                candidate_name.as_str()
            )));
        }
        if step.is_multiple_of(256)
            && reference.summary() != reference.recompute_summary(&SyntheticFacts)
        {
            return Err(QualificationFailure::new(
                "synthetic reference recomputation mismatch",
            ));
        }
    }

    for cell in 0..BLOCK_SECTION_CELLS {
        let position = pos_from_usize(cell);
        if candidate.get(position) != reference.get(position) {
            return Err(QualificationFailure::new(format!(
                "{} synthetic final cell mismatch",
                candidate_name.as_str()
            )));
        }
    }
    Ok(operations)
}

fn validate_generated_facts() -> Result<(), QualificationFailure> {
    if STATE_MUTATION_FLAGS.len() != BLOCK_STATE_COUNT {
        return Err(QualificationFailure::new(
            "generated mutation-fact table length differs from state count",
        ));
    }

    for (index, expected) in STATE_MUTATION_FLAGS.iter().copied().enumerate() {
        let raw = u32::try_from(index)
            .map_err(|_| QualificationFailure::new("state index does not fit u32"))?;
        let state = BlockStateId::new(raw)
            .ok_or_else(|| QualificationFailure::new("generated state ID rejected"))?;
        let facts = GeneratedStateFacts.facts(state);
        let observed = u8::from(facts.non_air())
            | (u8::from(facts.counted_fluid()) << 1)
            | (u8::from(facts.random_block()) << 2)
            | (u8::from(facts.random_fluid()) << 3);
        if observed != expected {
            return Err(QualificationFailure::new(format!(
                "generated mutation facts mismatch at state {index}: table={expected} observed={observed}"
            )));
        }
    }
    Ok(())
}

fn traces_for(mode: QualificationMode) -> Vec<Trace> {
    let mut traces = vec![
        trace_all_air(),
        trace_one_cell_reversal(),
        trace_localized_churn(),
        trace_random_uniform(),
        trace_high_entropy(),
        trace_dead_palette_churn(),
        trace_boundary_16(),
        trace_boundary_256(),
    ];
    match mode {
        QualificationMode::Quick => traces.push(trace_long_seeded(0xD1B5_4A32_D192_ED03, 10_000)),
        QualificationMode::Full => {
            for seed in [
                0xD1B5_4A32_D192_ED03,
                0xA24B_AED4_963E_E407,
                0x9E37_79B9_7F4A_7C15,
                0xBF58_476D_1CE4_E5B9,
                0x94D0_49BB_1331_11EB,
                0x2545_F491_4F6C_DD1D,
                0x369D_EA0F_31A5_3F85,
                0xDB4F_0B91_75AE_2165,
            ] {
                traces.push(trace_long_seeded(seed, 250_000));
            }
        }
    }
    traces
}

fn trace_all_air() -> Trace {
    let mut trace = Trace::new(TraceClass::AllAir, 0);
    trace
        .operations
        .extend([TraceOp::Summary, TraceOp::Contains(0), TraceOp::Contains(1)]);
    for cell in 0_u16..64 {
        trace.operations.push(TraceOp::Get(cell));
        trace.operations.push(TraceOp::ReplaceSame(cell));
    }
    trace.operations.push(TraceOp::Checkpoint);
    trace
}

fn trace_one_cell_reversal() -> Trace {
    let mut trace = Trace::new(TraceClass::OneCellReversal, 0x11CE_11CE);
    for cell in 0_u16..128 {
        let state = 1 + (cell % 31);
        trace.operations.push(TraceOp::Replace { cell, state });
        trace.operations.push(TraceOp::Get(cell));
        trace.operations.push(TraceOp::ReplaceSame(cell));
        trace.operations.push(TraceOp::Replace { cell, state: 0 });
    }
    trace.operations.push(TraceOp::Checkpoint);
    trace
}

fn trace_localized_churn() -> Trace {
    let seed = 0x10CA_11CE_D00D_u64;
    let mut trace = Trace::new(TraceClass::LocalizedChurn, seed);
    let mut rng = seed;
    for step in 0..2048_usize {
        let cell = bounded_u16(next_rng(&mut rng), 64);
        let state = bounded_u16(next_rng(&mut rng), 16);
        trace.operations.push(TraceOp::Replace { cell, state });
        if step.is_multiple_of(128) {
            trace.operations.push(TraceOp::Summary);
            trace.operations.push(TraceOp::Checkpoint);
        }
    }
    trace
}

fn trace_random_uniform() -> Trace {
    let seed = 0x000A_11CE_5EED_u64;
    let mut trace = Trace::new(TraceClass::RandomUniformWrites, seed);
    let mut rng = seed;
    for _round in 0..16 {
        let state = target_state(&mut rng);
        for _ in 0..64 {
            let cell = section_cell(&mut rng);
            trace.operations.push(TraceOp::Replace { cell, state });
        }
        trace.operations.push(TraceOp::Summary);
        trace.operations.push(TraceOp::Checkpoint);
    }
    trace
}

fn trace_high_entropy() -> Trace {
    let seed = 0x00E1_1720_F1E5_u64;
    let mut trace = Trace::new(TraceClass::HighEntropyWrites, seed);
    let mut rng = seed;
    for step in 0..4096_usize {
        trace.operations.push(TraceOp::Replace {
            cell: section_cell(&mut rng),
            state: target_state(&mut rng),
        });
        if step.is_multiple_of(256) {
            trace
                .operations
                .push(TraceOp::Contains(target_state(&mut rng)));
            trace.operations.push(TraceOp::Checkpoint);
        }
    }
    trace
}

fn trace_dead_palette_churn() -> Trace {
    let mut trace = Trace::new(TraceClass::DeadPaletteChurn, 0xDEAD_5107);
    for state in 1_u16..=255 {
        trace.operations.push(TraceOp::Replace {
            cell: state - 1,
            state,
        });
    }
    trace.operations.push(TraceOp::Checkpoint);
    for step in 0_u16..512 {
        let cell = step % 255;
        let state = 256 + step;
        trace.operations.push(TraceOp::Replace { cell, state });
        if step.is_multiple_of(64) {
            trace.operations.push(TraceOp::Contains(state));
            trace.operations.push(TraceOp::Checkpoint);
        }
    }
    trace
}

fn trace_boundary_16() -> Trace {
    let mut trace = Trace::new(TraceClass::Boundary16, 16);
    for state in 1_u16..=15 {
        trace.operations.push(TraceOp::Replace {
            cell: state - 1,
            state,
        });
    }
    trace.operations.extend([
        TraceOp::Checkpoint,
        TraceOp::ReplaceSame(14),
        TraceOp::Replace {
            cell: 15,
            state: 16,
        },
        TraceOp::Checkpoint,
        TraceOp::Replace { cell: 15, state: 0 },
        TraceOp::Checkpoint,
    ]);
    trace
}

fn trace_boundary_256() -> Trace {
    let mut trace = Trace::new(TraceClass::Boundary256, 256);
    for state in 1_u16..=255 {
        trace.operations.push(TraceOp::Replace {
            cell: state - 1,
            state,
        });
    }
    trace.operations.extend([
        TraceOp::Checkpoint,
        TraceOp::ReplaceSame(254),
        TraceOp::Replace {
            cell: 255,
            state: 256,
        },
        TraceOp::Checkpoint,
        TraceOp::Replace {
            cell: 255,
            state: 0,
        },
        TraceOp::Checkpoint,
    ]);
    trace
}

fn trace_long_seeded(seed: u64, mutations: usize) -> Trace {
    let mut trace = Trace::new(TraceClass::LongSeeded, seed);
    let mut rng = seed;
    for step in 0..mutations {
        let cell = section_cell(&mut rng);
        if step.is_multiple_of(257) {
            trace.operations.push(TraceOp::ReplaceSame(cell));
        } else {
            trace.operations.push(TraceOp::Replace {
                cell,
                state: target_state(&mut rng),
            });
        }
        if step.is_multiple_of(1024) {
            trace
                .operations
                .push(TraceOp::Contains(target_state(&mut rng)));
            trace.operations.push(TraceOp::Summary);
        }
        if step.is_multiple_of(2048) {
            trace.operations.push(TraceOp::Checkpoint);
        }
    }
    trace
}

fn next_rng(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn section_cell(rng: &mut u64) -> u16 {
    bounded_u16(next_rng(rng), BLOCK_SECTION_CELLS)
}

fn target_state(rng: &mut u64) -> u16 {
    bounded_u16(next_rng(rng), BLOCK_STATE_COUNT)
}

fn bounded_u16(value: u64, bound: usize) -> u16 {
    let bound = u64::try_from(bound).expect("qualification bounds fit u64");
    let bounded = value % bound;
    u16::try_from(bounded).expect("qualification bound fits u16")
}

fn state_id(raw: u16) -> BlockStateId {
    BlockStateId::new(u32::from(raw))
        .expect("trace decoder/generator guarantees target state range")
}

fn pos(cell: u16) -> SectionBlockPos {
    pos_from_usize(usize::from(cell))
}

fn pos_from_usize(cell: usize) -> SectionBlockPos {
    debug_assert!(cell < BLOCK_SECTION_CELLS);
    let x = u8::try_from(cell & 15).expect("bounded x");
    let z = u8::try_from((cell >> 4) & 15).expect("bounded z");
    let y = u8::try_from((cell >> 8) & 15).expect("bounded y");
    SectionBlockPos::new(x, y, z).expect("decoded section position")
}

fn parse_cell(value: &str, line: usize) -> Result<u16, QualificationFailure> {
    let cell = value
        .parse::<u16>()
        .map_err(|_| QualificationFailure::new(format!("invalid cell at line {line}")))?;
    if usize::from(cell) >= BLOCK_SECTION_CELLS {
        return Err(QualificationFailure::new(format!(
            "cell out of range at line {line}"
        )));
    }
    Ok(cell)
}

fn parse_state(value: &str, line: usize) -> Result<u16, QualificationFailure> {
    let state = value
        .parse::<u16>()
        .map_err(|_| QualificationFailure::new(format!("invalid state at line {line}")))?;
    if usize::from(state) >= BLOCK_STATE_COUNT {
        return Err(QualificationFailure::new(format!(
            "state out of range at line {line}"
        )));
    }
    Ok(state)
}

fn trace_failure<T>(
    candidate: Candidate,
    trace: &Trace,
    operation_index: usize,
    detail: &str,
) -> Result<T, QualificationFailure> {
    Err(QualificationFailure::new(format!(
        "candidate={} trace={} seed={:016x} operation={} {detail}",
        candidate.as_str(),
        trace.class.as_str(),
        trace.seed,
        operation_index
    )))
}

fn trace_fingerprint(traces: &[Trace]) -> u64 {
    let mut hash = FNV_OFFSET;
    for trace in traces {
        for byte in trace.encode().bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

fn json_safe_token(value: &str) -> &str {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        value
    } else {
        "invalid-token"
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Candidate, QualificationMode, Trace, TraceClass, TraceOp, qualify, trace_long_seeded,
        traces_for,
    };

    #[test]
    fn trace_format_round_trips_all_operation_kinds() {
        let trace = Trace {
            class: TraceClass::Boundary16,
            seed: 0x1234,
            operations: vec![
                TraceOp::Get(0),
                TraceOp::Replace { cell: 1, state: 2 },
                TraceOp::ReplaceSame(1),
                TraceOp::Summary,
                TraceOp::Contains(2),
                TraceOp::Checkpoint,
            ],
        };
        let encoded = trace.encode();
        assert_eq!(Trace::decode(&encoded).expect("valid trace"), trace);
    }

    #[test]
    fn generated_quick_suite_is_deterministic() {
        let first = traces_for(QualificationMode::Quick)
            .iter()
            .map(Trace::encode)
            .collect::<Vec<_>>();
        let second = traces_for(QualificationMode::Quick)
            .iter()
            .map(Trace::encode)
            .collect::<Vec<_>>();
        assert_eq!(first, second);
    }

    #[test]
    fn long_trace_seed_changes_stream() {
        assert_ne!(
            trace_long_seeded(1, 1024).encode(),
            trace_long_seeded(2, 1024).encode()
        );
    }

    #[test]
    fn quick_direct_qualification_is_green() {
        let report = qualify(QualificationMode::Quick, Some(Candidate::Direct))
            .expect("direct candidate must qualify");
        assert_eq!(report.records().len(), 1);
        assert_eq!(report.records()[0].candidate(), Candidate::Direct);
        assert!(report.records()[0].trace_operations() > 10_000);
    }
}
