//! Deterministic 1/2/4-worker baseline for Crucible's admitted ownership model.
//!
//! This is qualification infrastructure, not a production scheduler. Workers own disjoint real
//! block sections and execute statically partitioned work. Cross-domain semantic effects are
//! collected at an explicit stage barrier and committed by the deterministic ownership oracle.
//! Worker completion order is therefore never gameplay order.

#![forbid(unsafe_code)]

use std::fs;
use std::mem::size_of;
use std::ops::Range;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;

use crucible_generated::{
    AIR, BLOCK_STATE_COUNT, BlockStateId, GeneratedStateFacts, STATE_DATA_GENERATION_SHA256,
    STATE_DATA_INPUT_SHA256,
};
use crucible_ownership_qualification::{
    DomainId, EffectId, EffectPayload, OwnershipSimulator, SemanticDigest, WorkerId,
};
use crucible_world_contract::{BLOCK_SECTION_CELLS, BlockSection, SectionBlockPos};
use crucible_world_reference::DirectBlockSection;

/// Target-state input digest bound into this baseline's semantic work.
pub const TARGET_STATE_INPUT_SHA256: &str = STATE_DATA_INPUT_SHA256;
/// Generated target-state digest bound into this baseline's semantic work.
pub const TARGET_STATE_GENERATION_SHA256: &str = STATE_DATA_GENERATION_SHA256;

const CHECKSUM_SEED: u64 = 0x9E37_79B9_7F4A_7C15;
const CHECKSUM_MUL: u64 = 0xD6E8_FEB8_6659_FD93;

#[derive(Clone, Debug)]
struct DomainWork {
    domain: DomainId,
    section: DirectBlockSection<BlockStateId>,
    trace: Vec<SectionBlockPos>,
    even_stage_state: BlockStateId,
    odd_stage_state: BlockStateId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DomainOutcome {
    domain: DomainId,
    worker: WorkerId,
    local_delta: i64,
    cross_delta: i64,
    checksum: u64,
}

#[derive(Debug)]
struct StageBatch {
    worker: WorkerId,
    outcomes: Vec<DomainOutcome>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Control {
    Continue,
    Stop,
    Abort,
}

/// Predeclared real-section workload shared by every worker-count candidate.
///
/// Cloning this value is fixture preparation and should happen outside a timed region. Execution
/// itself consumes the clone so each candidate receives an independent but identical semantic
/// starting image.
#[derive(Clone, Debug)]
pub struct PreparedWorkload {
    domains: Vec<DomainWork>,
    stages: usize,
    operations_per_domain: usize,
}

/// Topology-independent result of one baseline executor run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunEvidence {
    /// Number of physical worker threads used by the baseline mechanism.
    pub workers: usize,
    /// Digest captured after every completed semantic stage.
    pub stage_digests: Vec<SemanticDigest>,
    /// Deterministic checksum of all useful section work, independent of worker topology.
    pub work_checksum: u64,
    /// Count of declared section operations performed across all domains and stages.
    pub useful_operations: u64,
}

/// Deterministic logical memory attributed to the prepared baseline workload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalMemory {
    /// Backing bytes for all 4096-cell block-state arrays.
    pub section_cell_bytes: usize,
    /// Shallow bytes for the per-domain work records, excluding owned buffers counted separately.
    pub domain_shallow_bytes: usize,
    /// Bytes in the predeclared per-domain position traces.
    pub trace_bytes: usize,
    /// Maximum bytes in one stage's topology-independent outcome vector.
    pub stage_outcome_bytes: usize,
    /// Shallow bytes for the static worker partition vectors.
    pub worker_partition_shallow_bytes: usize,
}

impl LogicalMemory {
    /// Sum of the deterministic categories measured by this logical model.
    #[must_use]
    pub const fn total_accounted_bytes(self) -> usize {
        self.section_cell_bytes
            + self.domain_shallow_bytes
            + self.trace_bytes
            + self.stage_outcome_bytes
            + self.worker_partition_shallow_bytes
    }
}

/// Best-effort Linux process-memory snapshot in KiB.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessMemory {
    /// Current resident set from `/proc/self/status` `VmRSS`, when available.
    pub rss_kib: Option<u64>,
    /// Process high-water resident set from `/proc/self/status` `VmHWM`, when available.
    pub hwm_kib: Option<u64>,
}

impl PreparedWorkload {
    /// Builds deterministic real-section work outside the benchmark timing region.
    ///
    /// # Errors
    ///
    /// Returns an error for zero/oversized domain counts, zero stages/operations, coordinate
    /// construction failure, or an impossible generated block-state identity.
    pub fn new(
        domain_count: usize,
        stages: usize,
        operations_per_domain: usize,
    ) -> Result<Self, String> {
        if domain_count == 0 {
            return Err("domain count must be positive".to_owned());
        }
        if domain_count > usize::from(u16::MAX) {
            return Err("domain count exceeds DomainId capacity".to_owned());
        }
        if stages == 0 {
            return Err("stage count must be positive".to_owned());
        }
        if operations_per_domain == 0 {
            return Err("operations per domain must be positive".to_owned());
        }

        let mut domains = Vec::with_capacity(domain_count);
        for raw_domain in 0..domain_count {
            let domain_raw = u16::try_from(raw_domain)
                .map_err(|_| "domain identity does not fit u16".to_owned())?;
            let domain = DomainId(domain_raw);
            domains.push(build_domain(domain, operations_per_domain)?);
        }
        Ok(Self {
            domains,
            stages,
            operations_per_domain,
        })
    }

    /// Number of independently owned semantic domains in this workload.
    #[must_use]
    pub fn domain_count(&self) -> usize {
        self.domains.len()
    }

    /// Number of semantic stages in one execution.
    #[must_use]
    pub const fn stages(&self) -> usize {
        self.stages
    }

    /// Useful section operations performed by one domain in one stage.
    #[must_use]
    pub const fn operations_per_domain(&self) -> usize {
        self.operations_per_domain
    }

    /// Returns deterministic logical memory for this workload and one static worker partitioning.
    ///
    /// This intentionally does not guess allocator metadata, channel internals, thread stacks or
    /// resident-page behavior. Those belong in process measurements and platform provenance.
    #[must_use]
    pub fn logical_memory(&self, workers: usize) -> LogicalMemory {
        LogicalMemory {
            section_cell_bytes: self
                .domains
                .len()
                .saturating_mul(BLOCK_SECTION_CELLS)
                .saturating_mul(size_of::<BlockStateId>()),
            domain_shallow_bytes: self.domains.len().saturating_mul(size_of::<DomainWork>()),
            trace_bytes: self
                .domains
                .len()
                .saturating_mul(self.operations_per_domain)
                .saturating_mul(size_of::<SectionBlockPos>()),
            stage_outcome_bytes: self
                .domains
                .len()
                .saturating_mul(size_of::<DomainOutcome>()),
            worker_partition_shallow_bytes: workers.saturating_mul(size_of::<Vec<DomainWork>>()),
        }
    }

    /// Executes the prepared workload with a persistent statically partitioned thread set.
    ///
    /// Every worker exclusively owns its `DirectBlockSection` values. After each stage it sends one
    /// bounded batch to the coordinator and waits for an explicit release. The coordinator sorts all
    /// domain outcomes by `DomainId`, applies mutations/effects through `OwnershipSimulator`, records
    /// a topology-independent digest, then releases the next stage. No mutex protects section data.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid worker count, channel/thread failure, an ownership-oracle
    /// rejection, duplicate/missing worker batches, or arithmetic overflow in evidence counters.
    pub fn execute(self, workers: usize) -> Result<RunEvidence, String> {
        let ranges = partition_ranges(self.domains.len(), workers)?;
        let domain_count = self.domains.len();
        let domain_count_u16 =
            u16::try_from(domain_count).map_err(|_| "domain count does not fit u16".to_owned())?;

        let mut owners = Vec::with_capacity(domain_count);
        for (worker_index, range) in ranges.iter().enumerate() {
            let worker_raw = u16::try_from(worker_index)
                .map_err(|_| "worker identity does not fit u16".to_owned())?;
            for raw_domain in range.clone() {
                let domain_raw = u16::try_from(raw_domain)
                    .map_err(|_| "domain identity does not fit u16".to_owned())?;
                owners.push((
                    DomainId(domain_raw),
                    WorkerId(worker_raw),
                    i64::from(domain_raw),
                ));
            }
        }
        owners.sort_by_key(|(domain, _, _)| *domain);
        let mut simulator = OwnershipSimulator::new(owners)
            .map_err(|error| format!("ownership simulator construction failed: {error:?}"))?;

        let mut domain_iter = self.domains.into_iter();
        let mut partitions = Vec::with_capacity(workers);
        for range in &ranges {
            partitions.push(domain_iter.by_ref().take(range.len()).collect::<Vec<_>>());
        }
        if domain_iter.next().is_some() {
            return Err("static partition failed to consume every domain".to_owned());
        }

        let stages = self.stages;
        let operations_per_domain = self.operations_per_domain;
        let useful_operations = u64::try_from(domain_count)
            .ok()
            .and_then(|domains| {
                u64::try_from(stages)
                    .ok()
                    .and_then(|stage_count| domains.checked_mul(stage_count))
            })
            .and_then(|domain_stages| {
                u64::try_from(operations_per_domain)
                    .ok()
                    .and_then(|operations| domain_stages.checked_mul(operations))
            })
            .ok_or_else(|| "useful operation count overflow".to_owned())?;

        let (stage_digests, work_checksum) = thread::scope(|scope| {
            let (batch_tx, batch_rx) = mpsc::sync_channel::<StageBatch>(workers);
            let mut control_txs = Vec::with_capacity(workers);
            let mut handles = Vec::with_capacity(workers);

            for (worker_index, partition) in partitions.into_iter().enumerate() {
                let worker_raw = u16::try_from(worker_index)
                    .map_err(|_| "worker identity does not fit u16".to_owned())?;
                let worker = WorkerId(worker_raw);
                let tx = batch_tx.clone();
                let (control_tx, control_rx) = mpsc::sync_channel::<Control>(1);
                control_txs.push(control_tx);
                handles.push(
                    scope.spawn(move || worker_loop(worker, partition, stages, &tx, &control_rx)),
                );
            }
            drop(batch_tx);

            let coordinator = coordinate_stages(
                &mut simulator,
                stages,
                workers,
                domain_count_u16,
                &batch_rx,
                &control_txs,
            );

            if coordinator.is_err() {
                for control in &control_txs {
                    let _ = control.try_send(Control::Abort);
                }
            }

            let mut worker_error = None;
            for handle in handles {
                match handle.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        worker_error.get_or_insert(error);
                    }
                    Err(_) => {
                        worker_error.get_or_insert_with(|| "baseline worker panicked".to_owned());
                    }
                }
            }

            match (coordinator, worker_error) {
                (Ok(result), None) => Ok(result),
                (Err(error), _) | (Ok(_), Some(error)) => Err(error),
            }
        })?;

        Ok(RunEvidence {
            workers,
            stage_digests,
            work_checksum,
            useful_operations,
        })
    }
}

fn worker_loop(
    worker: WorkerId,
    mut domains: Vec<DomainWork>,
    stages: usize,
    batch_tx: &SyncSender<StageBatch>,
    control_rx: &Receiver<Control>,
) -> Result<(), String> {
    for stage in 0..stages {
        let mut outcomes = Vec::with_capacity(domains.len());
        for domain in &mut domains {
            outcomes.push(process_domain_stage(domain, worker, stage)?);
        }
        batch_tx
            .send(StageBatch { worker, outcomes })
            .map_err(|_| "coordinator dropped stage receiver".to_owned())?;

        match control_rx
            .recv()
            .map_err(|_| "coordinator dropped worker control channel".to_owned())?
        {
            Control::Continue if stage + 1 < stages => {}
            Control::Stop if stage + 1 == stages => return Ok(()),
            Control::Abort => return Err("baseline coordinator aborted execution".to_owned()),
            Control::Continue | Control::Stop => {
                return Err("coordinator sent invalid stage control".to_owned());
            }
        }
    }
    Err("worker completed stages without final stop control".to_owned())
}

fn coordinate_stages(
    simulator: &mut OwnershipSimulator,
    stages: usize,
    workers: usize,
    domain_count: u16,
    batch_rx: &Receiver<StageBatch>,
    control_txs: &[SyncSender<Control>],
) -> Result<(Vec<SemanticDigest>, u64), String> {
    let mut stage_digests = Vec::with_capacity(stages);
    let mut work_checksum = CHECKSUM_SEED;

    for stage in 0..stages {
        let mut outcomes = Vec::new();
        let mut seen_workers = vec![false; workers];
        for _ in 0..workers {
            let batch = batch_rx
                .recv()
                .map_err(|_| "worker stage channel closed before barrier".to_owned())?;
            let worker_index = usize::from(batch.worker.0);
            if worker_index >= workers || seen_workers[worker_index] {
                return Err("duplicate or out-of-range worker stage batch".to_owned());
            }
            seen_workers[worker_index] = true;
            outcomes.extend(batch.outcomes);
        }
        if seen_workers.iter().any(|seen| !seen) {
            return Err("stage barrier did not receive every worker".to_owned());
        }
        outcomes.sort_by_key(|outcome| outcome.domain);
        if outcomes.len() != usize::from(domain_count) {
            return Err("stage barrier did not receive every domain".to_owned());
        }

        let stage_u64 = u64::try_from(stage).map_err(|_| "stage identity overflow".to_owned())?;
        for outcome in &outcomes {
            let token = simulator
                .token(outcome.domain)
                .map_err(|error| format!("could not acquire current authority: {error:?}"))?;
            if token.worker() != outcome.worker {
                return Err("worker executed a domain outside its static authority".to_owned());
            }
            simulator
                .mutate(token, outcome.local_delta)
                .map_err(|error| format!("local baseline mutation rejected: {error:?}"))?;

            let target_raw = if outcome.domain.0 + 1 == domain_count {
                0
            } else {
                outcome.domain.0 + 1
            };
            let effect_number = stage_u64
                .checked_mul(u64::from(domain_count))
                .and_then(|base| base.checked_add(u64::from(outcome.domain.0)))
                .ok_or_else(|| "effect identity overflow".to_owned())?;
            simulator
                .emit_effect(
                    token,
                    EffectId(effect_number),
                    DomainId(target_raw),
                    EffectPayload::Add(outcome.cross_delta),
                )
                .map_err(|error| format!("cross-domain baseline effect rejected: {error:?}"))?;

            work_checksum = mix_checksum(work_checksum, u64::from(outcome.domain.0));
            work_checksum = mix_checksum(work_checksum, outcome.checksum);
        }

        simulator
            .finish_stage()
            .map_err(|error| format!("stage barrier commit failed: {error:?}"))?;
        stage_digests.push(
            simulator
                .semantic_digest()
                .map_err(|error| format!("semantic digest failed: {error:?}"))?,
        );

        let control = if stage + 1 == stages {
            Control::Stop
        } else {
            simulator
                .begin_stage()
                .map_err(|error| format!("next stage could not open: {error:?}"))?;
            Control::Continue
        };
        for tx in control_txs {
            tx.send(control)
                .map_err(|_| "worker dropped before stage release".to_owned())?;
        }
    }

    Ok((stage_digests, work_checksum))
}

fn process_domain_stage(
    domain: &mut DomainWork,
    worker: WorkerId,
    stage: usize,
) -> Result<DomainOutcome, String> {
    let write_state = if stage.is_multiple_of(2) {
        domain.even_stage_state
    } else {
        domain.odd_stage_state
    };
    let mut checksum = CHECKSUM_SEED ^ u64::from(domain.domain.0);

    for (operation, &pos) in domain.trace.iter().enumerate() {
        let current = domain.section.get(pos);
        let current_raw = u64::try_from(current.as_usize())
            .map_err(|_| "block-state identity does not fit u64".to_owned())?;
        checksum = mix_checksum(checksum, current_raw);
        if operation.is_multiple_of(16) {
            let previous = domain
                .section
                .replace(pos, write_state, &GeneratedStateFacts);
            let previous_raw = u64::try_from(previous.as_usize())
                .map_err(|_| "block-state identity does not fit u64".to_owned())?;
            checksum = mix_checksum(checksum, previous_raw ^ 0xA5A5_A5A5_A5A5_A5A5);
        }
    }

    let summary = domain.section.summary();
    checksum = mix_checksum(checksum, u64::from(summary.non_air_count));
    checksum = mix_checksum(checksum, u64::from(summary.fluid_count));
    checksum = mix_checksum(checksum, u64::from(u8::from(summary.random_block_present)));
    checksum = mix_checksum(checksum, u64::from(u8::from(summary.random_fluid_present)));

    let stage_i64 = i64::try_from(stage).map_err(|_| "stage does not fit i64".to_owned())?;
    let local_delta = i64::from(domain.domain.0 % 7 + 1) + stage_i64.rem_euclid(3);
    let cross_delta = i64::from(domain.domain.0 % 5) - 2;
    Ok(DomainOutcome {
        domain: domain.domain,
        worker,
        local_delta,
        cross_delta,
        checksum,
    })
}

fn build_domain(domain: DomainId, operations: usize) -> Result<DomainWork, String> {
    let mut section = DirectBlockSection::filled(AIR, &GeneratedStateFacts);
    let state_universe = BLOCK_STATE_COUNT
        .checked_sub(1)
        .ok_or_else(|| "generated state universe contains no non-air state".to_owned())?;

    for cell in 0..BLOCK_SECTION_CELLS {
        let pos = position_from_cell(cell)?;
        let raw = (usize::from(domain.0)
            .wrapping_mul(4_099)
            .wrapping_add(cell.wrapping_mul(37))
            .wrapping_add(11))
            % state_universe
            + 1;
        let raw = u32::try_from(raw).map_err(|_| "fixture state does not fit u32".to_owned())?;
        let state = BlockStateId::new(raw)
            .ok_or_else(|| format!("fixture produced invalid target state {raw}"))?;
        let _ = section.replace(pos, state, &GeneratedStateFacts);
    }

    let mut trace = Vec::with_capacity(operations);
    let mut rng = CHECKSUM_SEED ^ (u64::from(domain.0) << 32);
    for _ in 0..operations {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        let cell = usize::try_from(rng % 4_096)
            .map_err(|_| "bounded fixture position does not fit usize".to_owned())?;
        trace.push(position_from_cell(cell)?);
    }

    Ok(DomainWork {
        domain,
        section,
        trace,
        even_stage_state: deterministic_state(domain, 17, state_universe)?,
        odd_stage_state: deterministic_state(domain, 29, state_universe)?,
    })
}

fn deterministic_state(
    domain: DomainId,
    salt: usize,
    state_universe: usize,
) -> Result<BlockStateId, String> {
    let raw = (usize::from(domain.0).wrapping_mul(257).wrapping_add(salt)) % state_universe + 1;
    let raw = u32::try_from(raw).map_err(|_| "write state does not fit u32".to_owned())?;
    BlockStateId::new(raw).ok_or_else(|| format!("invalid deterministic write state {raw}"))
}

fn position_from_cell(cell: usize) -> Result<SectionBlockPos, String> {
    if cell >= BLOCK_SECTION_CELLS {
        return Err(format!("section cell index out of range: {cell}"));
    }
    let x = u8::try_from(cell & 0x0f).map_err(|_| "x coordinate overflow".to_owned())?;
    let z = u8::try_from((cell >> 4) & 0x0f).map_err(|_| "z coordinate overflow".to_owned())?;
    let y = u8::try_from((cell >> 8) & 0x0f).map_err(|_| "y coordinate overflow".to_owned())?;
    SectionBlockPos::new(x, y, z).ok_or_else(|| "bounded section coordinate rejected".to_owned())
}

fn partition_ranges(domain_count: usize, workers: usize) -> Result<Vec<Range<usize>>, String> {
    if workers == 0 {
        return Err("worker count must be positive".to_owned());
    }
    if workers > domain_count {
        return Err("worker count cannot exceed domain count".to_owned());
    }
    if workers > usize::from(u16::MAX) {
        return Err("worker count exceeds WorkerId capacity".to_owned());
    }

    let base = domain_count / workers;
    let remainder = domain_count % workers;
    let mut cursor = 0;
    let mut ranges = Vec::with_capacity(workers);
    for worker in 0..workers {
        let length = base + usize::from(worker < remainder);
        let end = cursor + length;
        ranges.push(cursor..end);
        cursor = end;
    }
    debug_assert_eq!(cursor, domain_count);
    Ok(ranges)
}

const fn mix_checksum(seed: u64, value: u64) -> u64 {
    seed.rotate_left(11) ^ value.wrapping_mul(CHECKSUM_MUL)
}

/// Produces a stable numeric identity for a topology-independent semantic digest.
#[must_use]
pub fn semantic_digest_checksum(digest: &SemanticDigest) -> u64 {
    let mut checksum = mix_checksum(CHECKSUM_SEED, digest.completed_stages);
    for &(domain, value, generation, revision) in &digest.domains {
        checksum = mix_checksum(checksum, u64::from(domain.0));
        checksum = mix_checksum(checksum, value.cast_unsigned());
        checksum = mix_checksum(checksum, generation.0);
        checksum = mix_checksum(checksum, revision.0);
    }
    checksum
}

/// Parses Linux `/proc/self/status` text without performing I/O.
#[must_use]
pub fn parse_proc_status(contents: &str) -> ProcessMemory {
    ProcessMemory {
        rss_kib: status_kib(contents, "VmRSS:"),
        hwm_kib: status_kib(contents, "VmHWM:"),
    }
}

/// Reads a best-effort process-memory snapshot. Unsupported platforms return empty fields.
#[must_use]
pub fn read_process_memory() -> ProcessMemory {
    fs::read_to_string("/proc/self/status").map_or_else(
        |_| ProcessMemory::default(),
        |text| parse_proc_status(&text),
    )
}

fn status_kib(contents: &str, prefix: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let value = line.strip_prefix(prefix)?.trim();
        let mut fields = value.split_whitespace();
        let amount = fields.next()?.parse::<u64>().ok()?;
        match fields.next() {
            None | Some("kB") => Some(amount),
            Some(_) => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{PreparedWorkload, parse_proc_status, partition_ranges, semantic_digest_checksum};

    #[test]
    fn static_partition_is_balanced_and_exact() {
        let ranges = partition_ranges(10, 4).expect("valid partition");
        assert_eq!(ranges, [0..3, 3..6, 6..8, 8..10]);
        let flattened = ranges
            .into_iter()
            .flat_map(Iterator::collect::<Vec<_>>)
            .collect::<Vec<_>>();
        assert_eq!(flattened, (0..10).collect::<Vec<_>>());
        assert!(partition_ranges(4, 0).is_err());
        assert!(partition_ranges(3, 4).is_err());
    }

    #[test]
    fn one_two_four_workers_preserve_every_semantic_stage() {
        let workload = PreparedWorkload::new(8, 3, 512).expect("valid workload");
        let one = workload.clone().execute(1).expect("one worker");
        let two = workload.clone().execute(2).expect("two workers");
        let four = workload.execute(4).expect("four workers");

        assert_eq!(one.stage_digests, two.stage_digests);
        assert_eq!(one.stage_digests, four.stage_digests);
        assert_eq!(one.work_checksum, two.work_checksum);
        assert_eq!(one.work_checksum, four.work_checksum);
        assert_eq!(one.useful_operations, two.useful_operations);
        assert_eq!(one.useful_operations, four.useful_operations);
        assert_ne!(one.work_checksum, 0);
        assert_eq!(one.stage_digests.len(), 3);
        assert_ne!(
            semantic_digest_checksum(one.stage_digests.last().expect("final digest")),
            0
        );
    }

    #[test]
    fn repeated_execution_is_deterministic() {
        let workload = PreparedWorkload::new(4, 2, 256).expect("valid workload");
        let first = workload.clone().execute(2).expect("first execution");
        let second = workload.execute(2).expect("second execution");
        assert_eq!(first, second);
    }

    #[test]
    fn logical_memory_scales_only_declared_worker_partition_state() {
        let workload = PreparedWorkload::new(8, 2, 128).expect("valid workload");
        let one = workload.logical_memory(1);
        let four = workload.logical_memory(4);
        assert_eq!(one.section_cell_bytes, four.section_cell_bytes);
        assert_eq!(one.trace_bytes, four.trace_bytes);
        assert_eq!(one.domain_shallow_bytes, four.domain_shallow_bytes);
        assert_eq!(one.stage_outcome_bytes, four.stage_outcome_bytes);
        assert!(four.worker_partition_shallow_bytes > one.worker_partition_shallow_bytes);
    }

    #[test]
    fn proc_status_parser_extracts_kib_and_rejects_other_units() {
        let memory = parse_proc_status(
            "Name:\tcrucible\nVmHWM:\t  4567 kB\nVmRSS:\t  1234 kB\nThreads:\t4\n",
        );
        assert_eq!(memory.rss_kib, Some(1_234));
        assert_eq!(memory.hwm_kib, Some(4_567));

        let invalid = parse_proc_status("VmRSS: 10 MB\nVmHWM: unknown kB\n");
        assert_eq!(invalid.rss_kib, None);
        assert_eq!(invalid.hwm_kib, None);
    }
}
