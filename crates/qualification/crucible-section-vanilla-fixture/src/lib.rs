//! Provenance-bound vanilla fixture replay for block-section semantics.
//!
//! Deterministic generated traces live in `crucible-section-qualification`. This crate handles the
//! independent side of M0.3C: externally sourced fixture packs whose expectations are bound either
//! to reviewed Mojang source surfaces or to an executed official runtime probe.

#![forbid(unsafe_code)]

use std::fmt::{self, Write as _};

use crucible_generated::{
    AIR, BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256,
    STATE_DATA_INPUT_SHA256, STATE_MUTATION_FLAGS,
};
use crucible_section_qualification::{Candidate, DATA_VERSION, MINECRAFT_VERSION, PROTOCOL_VERSION};
use crucible_world_contract::{
    BLOCK_SECTION_CELLS, BlockSection, BlockStateFacts, SectionBlockPos, SectionSummary,
};
use crucible_world_reference::DirectBlockSection;
use crucible_world_section::{
    AdaptiveBlockSection, DirectNBlockSection, FastLocalBlockSection, PackedLocalBlockSection,
};

/// Stable fixture schema version.
pub const FIXTURE_SCHEMA: u32 = 1;
/// Stable external-fixture evidence schema version.
pub const FIXTURE_EVIDENCE_SCHEMA: u32 = 1;

const FIXTURE_MAGIC: &str = "CRUCIBLE-SECTION-FIXTURE";
const SOURCE_ARCHIVE_SHA256: &str =
    "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750";
const OFFICIAL_SERVER_SHA256: &str =
    "cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5";

const REQUIRED_SOURCE_BINDINGS: [SourceBinding; 8] = [
    SourceBinding::new(
        "VAR-WORLD-SECTION-001",
        "e0ebea2b3f3e027008f79c9318413c60fa62fe0fd8b2cbbedf1838b2fe5e6b07",
        "f3f9e8c8717fdc42fa43a0b6da08b19e75ae549af194878b820033dcbcaae893",
    ),
    SourceBinding::new(
        "VAR-WORLD-SECTION-003",
        "575b754e4e2b56d5e84ffd4118e655b40ee8fd64279300a897051a10742d0e62",
        "33b28394066f55df351d79b0e95bb5427612678305a8ede178de29bbe7acb169",
    ),
    SourceBinding::new(
        "VAR-WORLD-SECTION-004",
        "79157573d34d0575ca46f3b3517fd74babedb7ace8e4aa29c330cadadccc9965",
        "8a8d35468eeaf75b2e063e887b593733bf93c8a1ac3b763c6d5307d18166c825",
    ),
    SourceBinding::new(
        "VAR-WORLD-SECTION-005",
        "ccbf4725950b1ef3c6d55182b901b78220cf549afa35745aacf65bcbcac79a4e",
        "cc8bc6b7e4daa0d04a68472abe17e2c6599a01eb145f3fd0eccd5a2d6ae61e97",
    ),
    SourceBinding::new(
        "VAR-WORLD-SECTION-006",
        "e43d4fa555cf6051e246f98f2bf60c16f0ac6d165d11a22064e9f671815d7533",
        "2d3b57983d556818aa812fed34a235873beaf282ff81ce399bc51dc599e10887",
    ),
    SourceBinding::new(
        "VAR-WORLD-SECTION-007",
        "6e4ffeefd0859f385ced37dd778e0d821985794e3d5c195f4e2a98b6ae728d41",
        "39cc670af1bceae92d787ab01c1f2da3ba493bb49ae8279a99674946e9307b64",
    ),
    SourceBinding::new(
        "VAR-WORLD-SECTION-008",
        "3d01744c955e5878488349eaf2847a4016add1e970ecfbe0a08300794ad6ee21",
        "b62c7b4ced6043be3c4a0ca78a16c4cc91482b2049e532fa6ff3a25def64fb98",
    ),
    SourceBinding::new(
        "VAR-WORLD-SECTION-009",
        "19f4e623ab95a03ca898c66429f1d56ba0850e3371f69aeb8274138344297110",
        "6a4590a9e6a71651c9654db52011d6621c4e2bc6ae8579b6539d08be8553f3fc",
    ),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceBinding {
    id: &'static str,
    normalized_sha256: &'static str,
    body_sha256: &'static str,
}

impl SourceBinding {
    const fn new(
        id: &'static str,
        normalized_sha256: &'static str,
        body_sha256: &'static str,
    ) -> Self {
        Self {
            id,
            normalized_sha256,
            body_sha256,
        }
    }
}

/// Provenance class for an external fixture pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixtureProvenance {
    /// Expectations are derived from reviewed source obligations and source fingerprints.
    SourceReviewed,
    /// Expectations were captured by executing the pinned official Mojang runtime.
    RuntimeObserved,
}

impl FixtureProvenance {
    const fn from_name(value: &str) -> Option<Self> {
        match value {
            "source-reviewed" => Some(Self::SourceReviewed),
            "runtime-observed" => Some(Self::RuntimeObserved),
            _ => None,
        }
    }

    /// Stable evidence spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SourceReviewed => "source-reviewed",
            Self::RuntimeObserved => "runtime-observed",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum StateSelector {
    Air,
    ExactFlags(u8),
    ExactId(u16),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StateAlias {
    name: String,
    selector: StateSelector,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FixtureSourceBinding {
    id: String,
    normalized_sha256: String,
    body_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FixtureOp {
    Summary(SectionSummary),
    Replace {
        cell: u16,
        next: String,
        previous: String,
    },
    ReplaceSame {
        cell: u16,
        state: String,
    },
    Get {
        cell: u16,
        state: String,
    },
    Present(String),
    Checkpoint,
}

/// Parsed external section fixture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VanillaFixture {
    id: String,
    provenance: FixtureProvenance,
    metadata: Vec<(String, String)>,
    source_bindings: Vec<FixtureSourceBinding>,
    states: Vec<StateAlias>,
    operations: Vec<FixtureOp>,
}

impl VanillaFixture {
    /// Parses and validates the stable line-oriented fixture schema.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureFailure`] for malformed input, duplicate/unknown aliases, out-of-range
    /// values, mismatched target provenance, or incomplete source/runtime provenance bindings.
    pub fn parse(input: &str) -> Result<Self, FixtureFailure> {
        let mut lines = input.lines();
        let header = lines
            .next()
            .ok_or_else(|| FixtureFailure::new("fixture is empty"))?;
        let header_parts = header.split('|').collect::<Vec<_>>();
        if header_parts.len() != 4 || header_parts[0] != FIXTURE_MAGIC {
            return Err(FixtureFailure::new("invalid fixture header"));
        }
        let schema = parse_u32(header_parts[1], "fixture schema")?;
        if schema != FIXTURE_SCHEMA {
            return Err(FixtureFailure::new(format!(
                "unsupported fixture schema {schema}"
            )));
        }
        validate_token(header_parts[2], "fixture id")?;
        let provenance = FixtureProvenance::from_name(header_parts[3])
            .ok_or_else(|| FixtureFailure::new("unknown fixture provenance"))?;

        let mut fixture = Self {
            id: header_parts[2].to_owned(),
            provenance,
            metadata: Vec::new(),
            source_bindings: Vec::new(),
            states: Vec::new(),
            operations: Vec::new(),
        };

        for (offset, line) in lines.enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line_number = offset + 2;
            let parts = line.split('|').collect::<Vec<_>>();
            match parts.as_slice() {
                ["M", key, value] => {
                    validate_token(key, "metadata key")?;
                    fixture.insert_metadata(key, value, line_number)?;
                }
                ["V", id, normalized, body] => {
                    validate_token(id, "VAR id")?;
                    validate_sha256(normalized, "VAR normalized SHA-256")?;
                    validate_sha256(body, "VAR body SHA-256")?;
                    if fixture.source_bindings.iter().any(|binding| binding.id == *id) {
                        return Err(FixtureFailure::new(format!(
                            "duplicate source binding {id} at line {line_number}"
                        )));
                    }
                    fixture.source_bindings.push(FixtureSourceBinding {
                        id: (*id).to_owned(),
                        normalized_sha256: (*normalized).to_owned(),
                        body_sha256: (*body).to_owned(),
                    });
                }
                ["T", name, "air"] => fixture.insert_state(name, StateSelector::Air, line_number)?,
                ["T", name, "flags", raw] => {
                    let flags = parse_u8(raw, "state flags")?;
                    if flags > 15 {
                        return Err(FixtureFailure::new(format!(
                            "state flags exceed four-bit domain at line {line_number}"
                        )));
                    }
                    fixture.insert_state(name, StateSelector::ExactFlags(flags), line_number)?;
                }
                ["T", name, "id", raw] => {
                    let id = parse_u16(raw, "state id")?;
                    if usize::from(id) >= BLOCK_STATE_COUNT {
                        return Err(FixtureFailure::new(format!(
                            "state id out of target range at line {line_number}"
                        )));
                    }
                    fixture.insert_state(name, StateSelector::ExactId(id), line_number)?;
                }
                ["S", non_air, fluid, random_block, random_fluid] => {
                    fixture.operations.push(FixtureOp::Summary(SectionSummary {
                        non_air_count: parse_u16(non_air, "non-air count")?,
                        fluid_count: parse_u16(fluid, "fluid count")?,
                        random_block_present: parse_bool(random_block, "random-block gate")?,
                        random_fluid_present: parse_bool(random_fluid, "random-fluid gate")?,
                    }));
                }
                ["R", cell, next, previous] => {
                    let cell = parse_cell(cell, line_number)?;
                    fixture.require_alias(next, line_number)?;
                    fixture.require_alias(previous, line_number)?;
                    fixture.operations.push(FixtureOp::Replace {
                        cell,
                        next: (*next).to_owned(),
                        previous: (*previous).to_owned(),
                    });
                }
                ["N", cell, state] => {
                    let cell = parse_cell(cell, line_number)?;
                    fixture.require_alias(state, line_number)?;
                    fixture.operations.push(FixtureOp::ReplaceSame {
                        cell,
                        state: (*state).to_owned(),
                    });
                }
                ["G", cell, state] => {
                    let cell = parse_cell(cell, line_number)?;
                    fixture.require_alias(state, line_number)?;
                    fixture.operations.push(FixtureOp::Get {
                        cell,
                        state: (*state).to_owned(),
                    });
                }
                ["P", state] => {
                    fixture.require_alias(state, line_number)?;
                    fixture.operations.push(FixtureOp::Present((*state).to_owned()));
                }
                ["K"] => fixture.operations.push(FixtureOp::Checkpoint),
                _ => {
                    return Err(FixtureFailure::new(format!(
                        "invalid fixture record at line {line_number}"
                    )));
                }
            }
        }

        fixture.validate_provenance()?;
        if fixture.states.is_empty() {
            return Err(FixtureFailure::new("fixture has no state aliases"));
        }
        if fixture.operations.is_empty() {
            return Err(FixtureFailure::new("fixture has no observations"));
        }
        for state in &fixture.states {
            let _ = fixture.resolve_selector(&state.selector)?;
        }
        Ok(fixture)
    }

    /// Stable fixture identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// External provenance class.
    #[must_use]
    pub const fn provenance(&self) -> FixtureProvenance {
        self.provenance
    }

    fn insert_metadata(
        &mut self,
        key: &str,
        value: &str,
        line_number: usize,
    ) -> Result<(), FixtureFailure> {
        if self.metadata.iter().any(|(existing, _)| existing == key) {
            return Err(FixtureFailure::new(format!(
                "duplicate metadata key {key} at line {line_number}"
            )));
        }
        self.metadata.push((key.to_owned(), value.to_owned()));
        Ok(())
    }

    fn insert_state(
        &mut self,
        name: &str,
        selector: StateSelector,
        line_number: usize,
    ) -> Result<(), FixtureFailure> {
        validate_token(name, "state alias")?;
        if self.states.iter().any(|state| state.name == name) {
            return Err(FixtureFailure::new(format!(
                "duplicate state alias {name} at line {line_number}"
            )));
        }
        self.states.push(StateAlias {
            name: name.to_owned(),
            selector,
        });
        Ok(())
    }

    fn require_alias(&self, name: &str, line_number: usize) -> Result<(), FixtureFailure> {
        if self.states.iter().any(|state| state.name == name) {
            Ok(())
        } else {
            Err(FixtureFailure::new(format!(
                "unknown state alias {name} at line {line_number}"
            )))
        }
    }

    fn metadata(&self, key: &str) -> Result<&str, FixtureFailure> {
        self.metadata
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
            .ok_or_else(|| FixtureFailure::new(format!("missing fixture metadata {key}")))
    }

    fn validate_provenance(&self) -> Result<(), FixtureFailure> {
        require_equal(
            self.metadata("minecraft_version")?,
            MINECRAFT_VERSION,
            "Minecraft version",
        )?;
        require_equal(
            self.metadata("protocol_version")?,
            &PROTOCOL_VERSION.to_string(),
            "protocol version",
        )?;
        require_equal(
            self.metadata("data_version")?,
            &DATA_VERSION.to_string(),
            "data version",
        )?;
        require_equal(
            self.metadata("state_data_input_sha256")?,
            STATE_DATA_INPUT_SHA256,
            "state-data input digest",
        )?;
        require_equal(
            self.metadata("state_data_generation_sha256")?,
            STATE_DATA_GENERATION_SHA256,
            "state-data generation digest",
        )?;
        require_equal(
            self.metadata("official_server_sha256")?,
            OFFICIAL_SERVER_SHA256,
            "official server digest",
        )?;

        match self.provenance {
            FixtureProvenance::SourceReviewed => {
                require_equal(
                    self.metadata("source_archive_sha256")?,
                    SOURCE_ARCHIVE_SHA256,
                    "source archive digest",
                )?;
                for expected in REQUIRED_SOURCE_BINDINGS {
                    let binding = self
                        .source_bindings
                        .iter()
                        .find(|binding| binding.id == expected.id)
                        .ok_or_else(|| {
                            FixtureFailure::new(format!(
                                "missing required source binding {}",
                                expected.id
                            ))
                        })?;
                    require_equal(
                        &binding.normalized_sha256,
                        expected.normalized_sha256,
                        "VAR normalized digest",
                    )?;
                    require_equal(
                        &binding.body_sha256,
                        expected.body_sha256,
                        "VAR body digest",
                    )?;
                }
            }
            FixtureProvenance::RuntimeObserved => {
                let probe = self.metadata("runtime_probe")?;
                if probe.is_empty() {
                    return Err(FixtureFailure::new("runtime probe name is empty"));
                }
            }
        }
        Ok(())
    }

    fn resolve_alias(&self, name: &str) -> Result<BlockStateId, FixtureFailure> {
        let selector = self
            .states
            .iter()
            .find(|state| state.name == name)
            .map(|state| &state.selector)
            .ok_or_else(|| FixtureFailure::new(format!("unknown resolved alias {name}")))?;
        self.resolve_selector(selector)
    }

    fn resolve_selector(&self, selector: &StateSelector) -> Result<BlockStateId, FixtureFailure> {
        match selector {
            StateSelector::Air => Ok(AIR),
            StateSelector::ExactFlags(expected) => {
                let index = STATE_MUTATION_FLAGS
                    .iter()
                    .position(|flags| flags == expected)
                    .ok_or_else(|| {
                        FixtureFailure::new(format!(
                            "target state universe has no state with exact flags {expected}"
                        ))
                    })?;
                BlockStateId::new(u32::try_from(index).map_err(|_| {
                    FixtureFailure::new("state table index does not fit generated ID input")
                })?)
                .ok_or_else(|| FixtureFailure::new("resolved flag state is outside target domain"))
            }
            StateSelector::ExactId(raw) => BlockStateId::new(u32::from(*raw))
                .ok_or_else(|| FixtureFailure::new("fixture state ID is outside target domain")),
        }
    }
}

/// One external-fixture candidate record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureEvidenceRecord {
    candidate: Candidate,
    operations: usize,
}

impl FixtureEvidenceRecord {
    /// Candidate admitted by this fixture.
    #[must_use]
    pub const fn candidate(&self) -> Candidate {
        self.candidate
    }

    /// Number of external fixture observations executed.
    #[must_use]
    pub const fn operations(&self) -> usize {
        self.operations
    }
}

/// Successful external-fixture qualification report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureReport {
    fixture_id: String,
    provenance: FixtureProvenance,
    records: Vec<FixtureEvidenceRecord>,
}

impl FixtureReport {
    /// Fixture identifier represented by the report.
    #[must_use]
    pub fn fixture_id(&self) -> &str {
        &self.fixture_id
    }

    /// Evidence provenance represented by the report.
    #[must_use]
    pub const fn provenance(&self) -> FixtureProvenance {
        self.provenance
    }

    /// Candidate records.
    #[must_use]
    pub fn records(&self) -> &[FixtureEvidenceRecord] {
        &self.records
    }

    /// Emits stable evidence JSON for the executed fixture pack.
    #[must_use]
    pub fn to_json(&self, commit_sha: &str) -> String {
        let mut output = String::new();
        let _ = writeln!(output, "{{");
        let _ = writeln!(output, "  \"schema\": {FIXTURE_EVIDENCE_SCHEMA},");
        let _ = writeln!(output, "  \"qualification\": \"section-vanilla-fixture\",");
        let _ = writeln!(output, "  \"fixture_id\": \"{}\",", self.fixture_id);
        let _ = writeln!(
            output,
            "  \"provenance\": \"{}\",",
            self.provenance.as_str()
        );
        let _ = writeln!(output, "  \"commit_sha\": \"{}\",", safe_token(commit_sha));
        let _ = writeln!(output, "  \"minecraft_version\": \"{MINECRAFT_VERSION}\",");
        let _ = writeln!(output, "  \"protocol_version\": {PROTOCOL_VERSION},");
        let _ = writeln!(output, "  \"data_version\": {DATA_VERSION},");
        let _ = writeln!(output, "  \"state_count\": {BLOCK_STATE_COUNT},");
        let _ = writeln!(
            output,
            "  \"state_data_input_sha256\": \"{STATE_DATA_INPUT_SHA256}\","
        );
        let _ = writeln!(
            output,
            "  \"state_data_generation_sha256\": \"{STATE_DATA_GENERATION_SHA256}\","
        );
        let _ = writeln!(output, "  \"records\": [");
        for (index, record) in self.records.iter().enumerate() {
            let suffix = if index + 1 == self.records.len() { "" } else { "," };
            let _ = writeln!(output, "    {{");
            let _ = writeln!(
                output,
                "      \"id\": \"EQUIV-WORLD-SECTION-VANILLA-{}-{}\",",
                self.provenance.as_str().to_ascii_uppercase().replace('-', "_"),
                record.candidate.as_str().to_ascii_uppercase().replace('-', "_")
            );
            let _ = writeln!(
                output,
                "      \"candidate\": \"{}\",",
                record.candidate.as_str()
            );
            let _ = writeln!(output, "      \"operations\": {}", record.operations);
            let _ = writeln!(output, "    }}{suffix}");
        }
        let _ = writeln!(output, "  ]");
        let _ = writeln!(output, "}}");
        output
    }
}

/// External fixture parsing or equivalence failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixtureFailure {
    detail: String,
}

impl FixtureFailure {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for FixtureFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for FixtureFailure {}

trait CandidateFactory: BlockSection<BlockStateId> + Clone + Sized {
    fn filled(state: BlockStateId) -> Self;
}

impl CandidateFactory for DirectNBlockSection<BlockStateId> {
    fn filled(state: BlockStateId) -> Self {
        DirectNBlockSection::filled(state, &GeneratedStateFacts)
    }
}

impl CandidateFactory for AdaptiveBlockSection<BlockStateId> {
    fn filled(state: BlockStateId) -> Self {
        AdaptiveBlockSection::filled(state, &GeneratedStateFacts)
    }
}

impl CandidateFactory for FastLocalBlockSection<BlockStateId> {
    fn filled(state: BlockStateId) -> Self {
        FastLocalBlockSection::filled(state, &GeneratedStateFacts)
    }
}

impl CandidateFactory for PackedLocalBlockSection<BlockStateId> {
    fn filled(state: BlockStateId) -> Self {
        PackedLocalBlockSection::filled(state, &GeneratedStateFacts)
    }
}

/// Replays one external vanilla fixture against the reference and selected candidates.
///
/// # Errors
///
/// Returns [`FixtureFailure`] when the fixture fails provenance validation, an expected vanilla
/// observation disagrees with the permanent direct oracle, or any candidate diverges from the same
/// expected observation/reference state.
pub fn qualify_fixture(
    fixture: &VanillaFixture,
    candidate_filter: Option<Candidate>,
) -> Result<FixtureReport, FixtureFailure> {
    let candidates =
        candidate_filter.map_or_else(|| Candidate::ALL.to_vec(), |candidate| vec![candidate]);
    let mut records = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        match candidate {
            Candidate::Direct => run::<DirectNBlockSection<BlockStateId>>(fixture, candidate)?,
            Candidate::Adaptive => run::<AdaptiveBlockSection<BlockStateId>>(fixture, candidate)?,
            Candidate::FastLocal => {
                run::<FastLocalBlockSection<BlockStateId>>(fixture, candidate)?;
            }
            Candidate::PackedLocal => {
                run::<PackedLocalBlockSection<BlockStateId>>(fixture, candidate)?;
            }
        }
        records.push(FixtureEvidenceRecord {
            candidate,
            operations: fixture.operations.len(),
        });
    }
    Ok(FixtureReport {
        fixture_id: fixture.id.clone(),
        provenance: fixture.provenance,
        records,
    })
}

fn run<C: CandidateFactory>(
    fixture: &VanillaFixture,
    candidate_name: Candidate,
) -> Result<(), FixtureFailure> {
    let mut reference = DirectBlockSection::filled(AIR, &GeneratedStateFacts);
    let mut candidate = C::filled(AIR);

    for (operation_index, operation) in fixture.operations.iter().enumerate() {
        match operation {
            FixtureOp::Summary(expected) => {
                verify_summary(
                    fixture,
                    &candidate,
                    &reference,
                    *expected,
                    candidate_name,
                    operation_index,
                )?;
            }
            FixtureOp::Replace {
                cell,
                next,
                previous,
            } => {
                let position = pos(*cell);
                let next = fixture.resolve_alias(next)?;
                let previous = fixture.resolve_alias(previous)?;
                let reference_previous =
                    reference.replace(position, next, &GeneratedStateFacts);
                if reference_previous != previous {
                    return fixture_failure(
                        fixture,
                        candidate_name,
                        operation_index,
                        "source/runtime expected previous state disagrees with reference",
                    );
                }
                let candidate_previous =
                    candidate.replace(position, next, &GeneratedStateFacts);
                if candidate_previous != previous || candidate.get(position) != next {
                    return fixture_failure(
                        fixture,
                        candidate_name,
                        operation_index,
                        "candidate replacement result/state disagrees with fixture",
                    );
                }
            }
            FixtureOp::ReplaceSame { cell, state } => {
                let position = pos(*cell);
                let expected = fixture.resolve_alias(state)?;
                if reference.get(position) != expected || candidate.get(position) != expected {
                    return fixture_failure(
                        fixture,
                        candidate_name,
                        operation_index,
                        "same-state precondition disagrees with fixture",
                    );
                }
                let reference_summary = reference.summary();
                let candidate_summary = candidate.summary();
                if reference.replace(position, expected, &GeneratedStateFacts) != expected
                    || candidate.replace(position, expected, &GeneratedStateFacts) != expected
                    || reference.summary() != reference_summary
                    || candidate.summary() != candidate_summary
                {
                    return fixture_failure(
                        fixture,
                        candidate_name,
                        operation_index,
                        "same-state replacement was not a semantic no-op",
                    );
                }
            }
            FixtureOp::Get { cell, state } => {
                let expected = fixture.resolve_alias(state)?;
                let position = pos(*cell);
                if reference.get(position) != expected || candidate.get(position) != expected {
                    return fixture_failure(
                        fixture,
                        candidate_name,
                        operation_index,
                        "get observation disagrees with fixture",
                    );
                }
            }
            FixtureOp::Present(state) => {
                let expected = fixture.resolve_alias(state)?;
                let exact = (0..BLOCK_SECTION_CELLS)
                    .any(|cell| reference.get(pos_from_usize(cell)) == expected);
                if !exact {
                    return fixture_failure(
                        fixture,
                        candidate_name,
                        operation_index,
                        "fixture claims presence for an absent reference state",
                    );
                }
                if !candidate.maybe_contains(|state| state == expected) {
                    return fixture_failure(
                        fixture,
                        candidate_name,
                        operation_index,
                        "candidate maybe_contains produced a false negative",
                    );
                }
            }
            FixtureOp::Checkpoint => {
                checkpoint(
                    fixture,
                    &candidate,
                    &reference,
                    candidate_name,
                    operation_index,
                )?;
            }
        }
    }

    checkpoint(
        fixture,
        &candidate,
        &reference,
        candidate_name,
        fixture.operations.len(),
    )
}

fn verify_summary<C: BlockSection<BlockStateId>>(
    fixture: &VanillaFixture,
    candidate: &C,
    reference: &DirectBlockSection<BlockStateId>,
    expected: SectionSummary,
    candidate_name: Candidate,
    operation_index: usize,
) -> Result<(), FixtureFailure> {
    let recomputed = reference.recompute_summary(&GeneratedStateFacts);
    if reference.summary() != recomputed {
        return fixture_failure(
            fixture,
            candidate_name,
            operation_index,
            "reference incremental/recompute disagreement",
        );
    }
    if recomputed != expected {
        return fixture_failure(
            fixture,
            candidate_name,
            operation_index,
            "source/runtime summary expectation disagrees with reference",
        );
    }
    if candidate.summary() != expected {
        return fixture_failure(
            fixture,
            candidate_name,
            operation_index,
            "candidate summary disagrees with external fixture",
        );
    }
    Ok(())
}

fn checkpoint<C: BlockSection<BlockStateId>>(
    fixture: &VanillaFixture,
    candidate: &C,
    reference: &DirectBlockSection<BlockStateId>,
    candidate_name: Candidate,
    operation_index: usize,
) -> Result<(), FixtureFailure> {
    for cell in 0..BLOCK_SECTION_CELLS {
        let position = pos_from_usize(cell);
        if candidate.get(position) != reference.get(position) {
            return fixture_failure(
                fixture,
                candidate_name,
                operation_index,
                "checkpoint cell disagreement",
            );
        }
    }
    let recomputed = reference.recompute_summary(&GeneratedStateFacts);
    if reference.summary() != recomputed || candidate.summary() != recomputed {
        return fixture_failure(
            fixture,
            candidate_name,
            operation_index,
            "checkpoint summary disagreement",
        );
    }
    Ok(())
}

fn fixture_failure<T>(
    fixture: &VanillaFixture,
    candidate: Candidate,
    operation_index: usize,
    detail: &str,
) -> Result<T, FixtureFailure> {
    Err(FixtureFailure::new(format!(
        "fixture={} provenance={} candidate={} operation={} {detail}",
        fixture.id,
        fixture.provenance.as_str(),
        candidate.as_str(),
        operation_index
    )))
}

fn pos(cell: u16) -> SectionBlockPos {
    pos_from_usize(usize::from(cell))
}

fn pos_from_usize(cell: usize) -> SectionBlockPos {
    let x = u8::try_from(cell & 15).expect("bounded x");
    let z = u8::try_from((cell >> 4) & 15).expect("bounded z");
    let y = u8::try_from((cell >> 8) & 15).expect("bounded y");
    SectionBlockPos::new(x, y, z).expect("decoded section position")
}

fn parse_cell(value: &str, line_number: usize) -> Result<u16, FixtureFailure> {
    let cell = parse_u16(value, "cell")?;
    if usize::from(cell) >= BLOCK_SECTION_CELLS {
        return Err(FixtureFailure::new(format!(
            "cell out of range at line {line_number}"
        )));
    }
    Ok(cell)
}

fn parse_u8(value: &str, label: &str) -> Result<u8, FixtureFailure> {
    value
        .parse::<u8>()
        .map_err(|_| FixtureFailure::new(format!("invalid {label}: {value}")))
}

fn parse_u16(value: &str, label: &str) -> Result<u16, FixtureFailure> {
    value
        .parse::<u16>()
        .map_err(|_| FixtureFailure::new(format!("invalid {label}: {value}")))
}

fn parse_u32(value: &str, label: &str) -> Result<u32, FixtureFailure> {
    value
        .parse::<u32>()
        .map_err(|_| FixtureFailure::new(format!("invalid {label}: {value}")))
}

fn parse_bool(value: &str, label: &str) -> Result<bool, FixtureFailure> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(FixtureFailure::new(format!(
            "invalid {label}: expected 0 or 1"
        ))),
    }
}

fn validate_token(value: &str, label: &str) -> Result<(), FixtureFailure> {
    if !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        Ok(())
    } else {
        Err(FixtureFailure::new(format!("invalid {label}: {value}")))
    }
}

fn validate_sha256(value: &str, label: &str) -> Result<(), FixtureFailure> {
    if value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(FixtureFailure::new(format!("invalid {label}")))
    }
}

fn require_equal(actual: &str, expected: &str, label: &str) -> Result<(), FixtureFailure> {
    if actual == expected {
        Ok(())
    } else {
        Err(FixtureFailure::new(format!(
            "{label} mismatch: expected {expected}, got {actual}"
        )))
    }
}

fn safe_token(value: &str) -> &str {
    if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        value
    } else {
        "invalid-commit"
    }
}

#[cfg(test)]
mod tests {
    use super::{Candidate, FixtureProvenance, VanillaFixture, qualify_fixture};

    const SOURCE_FIXTURE: &str = include_str!(
        "../../../../vanilla/fixtures/world/section/26.2-source-reviewed-count-gates.fixture"
    );

    #[test]
    fn committed_source_fixture_parses_and_is_bound() {
        let fixture = VanillaFixture::parse(SOURCE_FIXTURE).expect("committed fixture must parse");
        assert_eq!(fixture.id(), "source-reviewed-count-gates-v1");
        assert_eq!(fixture.provenance(), FixtureProvenance::SourceReviewed);
    }

    #[test]
    fn committed_source_fixture_qualifies_direct_reference_path() {
        let fixture = VanillaFixture::parse(SOURCE_FIXTURE).expect("committed fixture must parse");
        let report = qualify_fixture(&fixture, Some(Candidate::Direct))
            .expect("source fixture must agree with direct candidate and oracle");
        assert_eq!(report.records().len(), 1);
        assert!(report.records()[0].operations() >= 20);
    }

    #[test]
    fn tampered_source_digest_fails_closed() {
        let tampered = SOURCE_FIXTURE.replace(
            "1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750",
            "0e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750",
        );
        let error = VanillaFixture::parse(&tampered).expect_err("tampered source digest must fail");
        assert!(error.to_string().contains("source archive digest mismatch"));
    }
}
