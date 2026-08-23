use std::env;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use crucible_benchmark_support::{collect_hardware_metadata, push_json_string};
use crucible_connection_core::{ConnectionLimits, EgressBuffer};
use crucible_packet_core::{PacketCodecError, PacketWriter};
use crucible_protocol_core::{WireError, encode_string, encode_var_int, var_int_len};

const SCHEMA: u32 = 1;
const MAX_UTF16_UNITS: usize = 32_768;
const MAX_BODY_BYTES: usize = 96 * 1_024;
const MAX_EGRESS_BYTES: usize = 192 * 1_024;
const CHECKSUM_SEED: u64 = 0x9e37_79b9_7f4a_7c15;
const CHECKSUM_MUL: u64 = 0xd6e8_feb8_6659_fd93;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Smoke,
    Full,
}

impl Mode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Full => "full",
        }
    }
}

#[derive(Clone, Debug)]
struct Config {
    mode: Mode,
    output: Option<PathBuf>,
    warmup_rounds: usize,
    measured_rounds: usize,
    iteration_scale: usize,
}

impl Config {
    const fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                warmup_rounds: 1,
                measured_rounds: 3,
                iteration_scale: 1,
            },
            Mode::Full => Self {
                mode,
                output: None,
                warmup_rounds: 4,
                measured_rounds: 24,
                iteration_scale: 10,
            },
        }
    }
}

#[derive(Clone, Debug)]
enum Payload {
    Ping(i64),
    Status(String),
    Metadata {
        port: u16,
        epoch: u64,
        enabled: bool,
        label: String,
        bytes: Vec<u8>,
    },
    Raw(Vec<u8>),
}

#[derive(Clone, Debug)]
struct BenchCase {
    name: &'static str,
    packet_id: i32,
    payload: Payload,
    base_iterations: usize,
}

#[derive(Clone, Copy, Debug)]
struct RoundSample {
    round: usize,
    reference_first: bool,
    reference_ns: u128,
    fused_ns: u128,
    reference_checksum: u64,
    fused_checksum: u64,
}

#[derive(Clone, Copy, Debug)]
struct Summary {
    p50_ns: u128,
    p95_ns: u128,
    p99_ns: u128,
    max_ns: u128,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FusedError {
    Packet(PacketCodecError),
    EgressLimitExceeded {
        queued: usize,
        frame_bytes: usize,
        maximum: usize,
    },
    InvalidConsume {
        requested: usize,
        available: usize,
    },
    LengthOverflow,
}

impl From<PacketCodecError> for FusedError {
    fn from(value: PacketCodecError) -> Self {
        Self::Packet(value)
    }
}

#[derive(Debug)]
struct FusedEgress {
    bytes: Vec<u8>,
    start: usize,
    limits: ConnectionLimits,
    finalize_moved_bytes: u64,
}

impl FusedEgress {
    fn new(limits: ConnectionLimits) -> Self {
        Self {
            bytes: Vec::new(),
            start: 0,
            limits,
            finalize_moved_bytes: 0,
        }
    }

    fn queued_len(&self) -> usize {
        self.bytes.len() - self.start
    }

    fn pending(&self) -> &[u8] {
        &self.bytes[self.start..]
    }

    const fn finalize_moved_bytes(&self) -> u64 {
        self.finalize_moved_bytes
    }

    fn consume_written(&mut self, bytes: usize) -> Result<(), FusedError> {
        let available = self.queued_len();
        if bytes > available {
            return Err(FusedError::InvalidConsume {
                requested: bytes,
                available,
            });
        }
        self.start += bytes;
        if self.start == self.bytes.len() {
            self.bytes.clear();
            self.start = 0;
        }
        Ok(())
    }

    fn queue_packet(
        &mut self,
        packet_id: i32,
        encode_payload: impl FnOnce(&mut DirectPacketWriter<'_>) -> Result<(), PacketCodecError>,
    ) -> Result<(), FusedError> {
        self.compact_before_packet();
        let queued_before = self.queued_len();
        let physical_before = self.bytes.len();
        let frame_start = physical_before;

        let available_after_placeholder = self
            .limits
            .max_egress_queued()
            .checked_sub(queued_before)
            .and_then(|available| available.checked_sub(1))
            .ok_or(FusedError::EgressLimitExceeded {
                queued: queued_before,
                frame_bytes: 1,
                maximum: self.limits.max_egress_queued(),
            })?;
        let maximum_body = self
            .limits
            .max_frame_body_len()
            .min(available_after_placeholder);
        if maximum_body == 0 {
            return Err(FusedError::EgressLimitExceeded {
                queued: queued_before,
                frame_bytes: 1,
                maximum: self.limits.max_egress_queued(),
            });
        }

        self.bytes.push(0);
        let body_start = self.bytes.len();
        let encode_result = {
            let mut writer = DirectPacketWriter {
                bytes: &mut self.bytes,
                body_start,
                maximum: maximum_body,
            };
            writer
                .write_var_int(packet_id)
                .and_then(|()| encode_payload(&mut writer))
        };
        if let Err(error) = encode_result {
            self.bytes.truncate(physical_before);
            return Err(error.into());
        }

        let body_len = self.bytes.len() - body_start;
        let signed_len = i32::try_from(body_len).map_err(|_| {
            self.bytes.truncate(physical_before);
            FusedError::Packet(PacketCodecError::Wire(
                WireError::LengthDoesNotFitVarInt { length: body_len },
            ))
        })?;
        let prefix_len = var_int_len(signed_len);
        debug_assert!((1..=3).contains(&prefix_len));
        let frame_bytes = prefix_len
            .checked_add(body_len)
            .ok_or(FusedError::LengthOverflow)?;
        let required = queued_before
            .checked_add(frame_bytes)
            .ok_or(FusedError::LengthOverflow)?;
        if required > self.limits.max_egress_queued() {
            self.bytes.truncate(physical_before);
            return Err(FusedError::EgressLimitExceeded {
                queued: queued_before,
                frame_bytes,
                maximum: self.limits.max_egress_queued(),
            });
        }

        let extra_prefix = prefix_len - 1;
        if extra_prefix != 0 {
            let body_end = self.bytes.len();
            self.bytes.resize(
                body_end
                    .checked_add(extra_prefix)
                    .ok_or(FusedError::LengthOverflow)?,
                0,
            );
            self.bytes
                .copy_within(body_start..body_end, body_start + extra_prefix);
            self.finalize_moved_bytes = self
                .finalize_moved_bytes
                .checked_add(u64::try_from(body_len).map_err(|_| FusedError::LengthOverflow)?)
                .ok_or(FusedError::LengthOverflow)?;
        }

        write_frame_prefix(signed_len, &mut self.bytes[frame_start..frame_start + prefix_len]);
        Ok(())
    }

    fn compact_before_packet(&mut self) {
        if self.start == 0 {
            return;
        }
        let active = self.queued_len();
        self.bytes.copy_within(self.start.., 0);
        self.bytes.truncate(active);
        self.start = 0;
    }
}

struct DirectPacketWriter<'a> {
    bytes: &'a mut Vec<u8>,
    body_start: usize,
    maximum: usize,
}

impl DirectPacketWriter<'_> {
    fn len(&self) -> usize {
        self.bytes.len() - self.body_start
    }

    fn remaining_capacity(&self) -> usize {
        self.maximum - self.len()
    }

    fn write_var_int(&mut self, value: i32) -> Result<(), PacketCodecError> {
        self.reserve_field(var_int_len(value))?;
        encode_var_int(value, self.bytes);
        Ok(())
    }

    fn write_u16(&mut self, value: u16) -> Result<(), PacketCodecError> {
        self.reserve_field(2)?;
        self.bytes.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    fn write_i64(&mut self, value: i64) -> Result<(), PacketCodecError> {
        self.reserve_field(8)?;
        self.bytes.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    fn write_u64(&mut self, value: u64) -> Result<(), PacketCodecError> {
        self.reserve_field(8)?;
        self.bytes.extend_from_slice(&value.to_be_bytes());
        Ok(())
    }

    fn write_bool(&mut self, value: bool) -> Result<(), PacketCodecError> {
        self.reserve_field(1)?;
        self.bytes.push(u8::from(value));
        Ok(())
    }

    fn write_string(
        &mut self,
        value: &str,
        max_utf16_units: usize,
    ) -> Result<(), PacketCodecError> {
        let signed_len = i32::try_from(value.len()).map_err(|_| {
            PacketCodecError::Wire(WireError::LengthDoesNotFitVarInt {
                length: value.len(),
            })
        })?;
        let encoded_len = var_int_len(signed_len)
            .checked_add(value.len())
            .ok_or(PacketCodecError::LengthOverflow)?;
        self.reserve_field(encoded_len)?;
        encode_string(value, max_utf16_units, self.bytes)?;
        Ok(())
    }

    fn write_bytes(&mut self, value: &[u8]) -> Result<(), PacketCodecError> {
        self.reserve_field(value.len())?;
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn reserve_field(&self, additional: usize) -> Result<(), PacketCodecError> {
        let attempted = self
            .len()
            .checked_add(additional)
            .ok_or(PacketCodecError::LengthOverflow)?;
        if attempted > self.maximum {
            return Err(PacketCodecError::PacketLimitExceeded {
                attempted,
                maximum: self.maximum,
            });
        }
        Ok(())
    }
}

trait CaseWriter {
    fn write_u16(&mut self, value: u16) -> Result<(), PacketCodecError>;
    fn write_i64(&mut self, value: i64) -> Result<(), PacketCodecError>;
    fn write_u64(&mut self, value: u64) -> Result<(), PacketCodecError>;
    fn write_bool(&mut self, value: bool) -> Result<(), PacketCodecError>;
    fn write_string(&mut self, value: &str, max_utf16_units: usize)
    -> Result<(), PacketCodecError>;
    fn write_bytes(&mut self, value: &[u8]) -> Result<(), PacketCodecError>;
}

impl CaseWriter for PacketWriter {
    fn write_u16(&mut self, value: u16) -> Result<(), PacketCodecError> {
        PacketWriter::write_u16(self, value)
    }

    fn write_i64(&mut self, value: i64) -> Result<(), PacketCodecError> {
        PacketWriter::write_i64(self, value)
    }

    fn write_u64(&mut self, value: u64) -> Result<(), PacketCodecError> {
        PacketWriter::write_u64(self, value)
    }

    fn write_bool(&mut self, value: bool) -> Result<(), PacketCodecError> {
        PacketWriter::write_bool(self, value)
    }

    fn write_string(
        &mut self,
        value: &str,
        max_utf16_units: usize,
    ) -> Result<(), PacketCodecError> {
        PacketWriter::write_string(self, value, max_utf16_units)
    }

    fn write_bytes(&mut self, value: &[u8]) -> Result<(), PacketCodecError> {
        PacketWriter::write_bytes(self, value)
    }
}

impl CaseWriter for DirectPacketWriter<'_> {
    fn write_u16(&mut self, value: u16) -> Result<(), PacketCodecError> {
        DirectPacketWriter::write_u16(self, value)
    }

    fn write_i64(&mut self, value: i64) -> Result<(), PacketCodecError> {
        DirectPacketWriter::write_i64(self, value)
    }

    fn write_u64(&mut self, value: u64) -> Result<(), PacketCodecError> {
        DirectPacketWriter::write_u64(self, value)
    }

    fn write_bool(&mut self, value: bool) -> Result<(), PacketCodecError> {
        DirectPacketWriter::write_bool(self, value)
    }

    fn write_string(
        &mut self,
        value: &str,
        max_utf16_units: usize,
    ) -> Result<(), PacketCodecError> {
        DirectPacketWriter::write_string(self, value, max_utf16_units)
    }

    fn write_bytes(&mut self, value: &[u8]) -> Result<(), PacketCodecError> {
        DirectPacketWriter::write_bytes(self, value)
    }
}

fn write_payload(writer: &mut impl CaseWriter, payload: &Payload) -> Result<(), PacketCodecError> {
    match payload {
        Payload::Ping(value) => writer.write_i64(*value),
        Payload::Status(value) => writer.write_string(value, MAX_UTF16_UNITS),
        Payload::Metadata {
            port,
            epoch,
            enabled,
            label,
            bytes,
        } => {
            writer.write_u16(*port)?;
            writer.write_u64(*epoch)?;
            writer.write_bool(*enabled)?;
            writer.write_string(label, MAX_UTF16_UNITS)?;
            writer.write_bytes(bytes)
        }
        Payload::Raw(bytes) => writer.write_bytes(bytes),
    }
}

fn limits() -> ConnectionLimits {
    ConnectionLimits::new(MAX_BODY_BYTES, MAX_EGRESS_BYTES, MAX_EGRESS_BYTES)
        .expect("qualification limits are coherent")
}

fn build_reference(case: &BenchCase) -> Result<Vec<u8>, String> {
    let limits = limits();
    let mut writer = PacketWriter::new(limits.max_frame_body_len())
        .map_err(|error| format!("reference writer construction failed: {error:?}"))?;
    writer
        .write_var_int(case.packet_id)
        .map_err(|error| format!("reference packet id failed: {error:?}"))?;
    write_payload(&mut writer, &case.payload)
        .map_err(|error| format!("reference payload failed: {error:?}"))?;
    let mut egress = EgressBuffer::new(limits);
    egress
        .queue_frame(writer.as_slice())
        .map_err(|error| format!("reference frame queue failed: {error:?}"))?;
    Ok(egress.pending().to_vec())
}

fn build_fused(case: &BenchCase) -> Result<(Vec<u8>, u64), String> {
    let mut egress = FusedEgress::new(limits());
    egress
        .queue_packet(case.packet_id, |writer| write_payload(writer, &case.payload))
        .map_err(|error| format!("fused packet queue failed: {error:?}"))?;
    Ok((egress.pending().to_vec(), egress.finalize_moved_bytes()))
}

fn benchmark_cases() -> Vec<BenchCase> {
    let medium = deterministic_bytes(1_024, 0x51a7_23c9);
    let large = deterministic_bytes(64 * 1_024, 0x93d2_1ea5);
    let prefix_128 = deterministic_bytes(126, 0x7731_aa19);
    vec![
        BenchCase {
            name: "ping-i64",
            packet_id: 0x01,
            payload: Payload::Ping(0x1122_3344_5566_7788),
            base_iterations: 20_000,
        },
        BenchCase {
            name: "status-short-string",
            packet_id: 0x02,
            payload: Payload::Status(
                "{\"version\":{\"name\":\"Crucible\"},\"description\":\"Same game. Different engine.\"}"
                    .to_owned(),
            ),
            base_iterations: 10_000,
        },
        BenchCase {
            name: "prefix-transition-128",
            packet_id: 0,
            payload: Payload::Raw(prefix_128),
            base_iterations: 10_000,
        },
        BenchCase {
            name: "metadata-1k",
            packet_id: 0x11,
            payload: Payload::Metadata {
                port: 25_565,
                epoch: 0x0123_4567_89ab_cdef,
                enabled: true,
                label: "crucible-metadata".to_owned(),
                bytes: medium,
            },
            base_iterations: 2_000,
        },
        BenchCase {
            name: "stream-64k",
            packet_id: 0x21,
            payload: Payload::Raw(large),
            base_iterations: 100,
        },
    ]
}

fn deterministic_bytes(len: usize, seed: u32) -> Vec<u8> {
    let mut state = seed;
    let mut bytes = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        bytes.push(state.to_le_bytes()[0]);
    }
    bytes
}

fn write_frame_prefix(value: i32, output: &mut [u8]) {
    debug_assert!(value >= 0);
    debug_assert_eq!(output.len(), var_int_len(value));
    let mut remaining = value.cast_unsigned();
    for (index, byte) in output.iter_mut().enumerate() {
        let low = (remaining & 0x7f).to_le_bytes()[0];
        remaining >>= 7;
        *byte = if index + 1 == output.len() {
            low
        } else {
            low | 0x80
        };
    }
    debug_assert_eq!(remaining, 0);
}

fn benchmark_reference(case: &BenchCase, iterations: usize) -> Result<(u128, u64), String> {
    let limits = limits();
    let mut egress = EgressBuffer::new(limits);
    let mut checksum = CHECKSUM_SEED;
    let start = Instant::now();
    for _ in 0..iterations {
        let mut writer = PacketWriter::new(limits.max_frame_body_len())
            .map_err(|error| format!("reference writer construction failed: {error:?}"))?;
        writer
            .write_var_int(case.packet_id)
            .map_err(|error| format!("reference packet id failed: {error:?}"))?;
        write_payload(&mut writer, &case.payload)
            .map_err(|error| format!("reference payload failed: {error:?}"))?;
        egress
            .queue_frame(writer.as_slice())
            .map_err(|error| format!("reference queue failed: {error:?}"))?;
        checksum = observe_pending(checksum, black_box(egress.pending()));
        let queued = egress.queued_len();
        egress
            .consume_written(queued)
            .map_err(|error| format!("reference drain failed: {error:?}"))?;
    }
    Ok((start.elapsed().as_nanos(), checksum))
}

fn benchmark_fused(case: &BenchCase, iterations: usize) -> Result<(u128, u64, u64), String> {
    let mut egress = FusedEgress::new(limits());
    let moved_before = egress.finalize_moved_bytes();
    let mut checksum = CHECKSUM_SEED;
    let start = Instant::now();
    for _ in 0..iterations {
        egress
            .queue_packet(case.packet_id, |writer| write_payload(writer, &case.payload))
            .map_err(|error| format!("fused queue failed: {error:?}"))?;
        checksum = observe_pending(checksum, black_box(egress.pending()));
        let queued = egress.queued_len();
        egress
            .consume_written(queued)
            .map_err(|error| format!("fused drain failed: {error:?}"))?;
    }
    let elapsed = start.elapsed().as_nanos();
    let moved = egress
        .finalize_moved_bytes()
        .checked_sub(moved_before)
        .ok_or_else(|| "fused move counter regressed".to_owned())?;
    Ok((elapsed, checksum, moved))
}

fn observe_pending(mut checksum: u64, pending: &[u8]) -> u64 {
    checksum = mix(checksum, u64::try_from(pending.len()).unwrap_or(u64::MAX));
    checksum = mix(checksum, u64::from(pending.first().copied().unwrap_or(0)));
    checksum = mix(checksum, u64::from(pending.last().copied().unwrap_or(0)));
    checksum
}

fn mix(current: u64, value: u64) -> u64 {
    current.rotate_left(9) ^ value.wrapping_mul(CHECKSUM_MUL)
}

fn run_case(case: &BenchCase, config: &Config) -> Result<(usize, usize, u64, Vec<RoundSample>), String> {
    let reference = build_reference(case)?;
    let (fused, moved_once) = build_fused(case)?;
    if reference != fused {
        return Err(format!("byte-equivalence failed for {}", case.name));
    }
    if reference.is_empty() {
        return Err(format!("{} produced an empty framed packet", case.name));
    }
    let body_len = reference
        .len()
        .checked_sub(var_int_len(i32::try_from(reference.len() - 1).unwrap_or(i32::MAX)))
        .unwrap_or(reference.len());
    let iterations = case
        .base_iterations
        .checked_mul(config.iteration_scale)
        .ok_or_else(|| "benchmark iteration count overflow".to_owned())?;

    for round in 0..config.warmup_rounds {
        if round.is_multiple_of(2) {
            let _ = benchmark_reference(case, iterations)?;
            let _ = benchmark_fused(case, iterations)?;
        } else {
            let _ = benchmark_fused(case, iterations)?;
            let _ = benchmark_reference(case, iterations)?;
        }
    }

    let mut samples = Vec::with_capacity(config.measured_rounds);
    for round in 0..config.measured_rounds {
        let reference_first = round.is_multiple_of(2);
        let (reference_ns, reference_checksum, fused_ns, fused_checksum) = if reference_first {
            let (reference_ns, reference_checksum) = benchmark_reference(case, iterations)?;
            let (fused_ns, fused_checksum, _) = benchmark_fused(case, iterations)?;
            (reference_ns, reference_checksum, fused_ns, fused_checksum)
        } else {
            let (fused_ns, fused_checksum, _) = benchmark_fused(case, iterations)?;
            let (reference_ns, reference_checksum) = benchmark_reference(case, iterations)?;
            (reference_ns, reference_checksum, fused_ns, fused_checksum)
        };
        if reference_checksum != fused_checksum {
            return Err(format!(
                "observable checksum diverged for {} in round {round}",
                case.name
            ));
        }
        samples.push(RoundSample {
            round,
            reference_first,
            reference_ns,
            fused_ns,
            reference_checksum,
            fused_checksum,
        });
    }
    Ok((body_len, iterations, moved_once, samples))
}

fn summarize(values: impl IntoIterator<Item = u128>) -> Result<Summary, String> {
    let mut values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        return Err("cannot summarize an empty sample set".to_owned());
    }
    values.sort_unstable();
    Ok(Summary {
        p50_ns: percentile(&values, 50),
        p95_ns: percentile(&values, 95),
        p99_ns: percentile(&values, 99),
        max_ns: values.last().copied().unwrap_or(0),
    })
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
}

fn main() {
    if let Err(error) = run() {
        eprintln!("egress fusion benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    let mut case_reports = Vec::new();
    for case in benchmark_cases() {
        let (body_len, iterations, moved_once, samples) = run_case(&case, &config)?;
        case_reports.push((case, body_len, iterations, moved_once, samples));
    }

    let report = render_report(&config, &hardware.to_json(), &case_reports)?;
    if let Some(path) = config.output {
        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        fs::write(&path, report)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        eprintln!("wrote {}", path.display());
    } else {
        println!("{report}");
    }
    Ok(())
}

fn render_report(
    config: &Config,
    hardware_json: &str,
    reports: &[(BenchCase, usize, usize, u64, Vec<RoundSample>)],
) -> Result<String, String> {
    let mut output = String::new();
    write!(output, "{{\n  \"schema\":{SCHEMA}").expect("writing to String cannot fail");
    output.push_str(",\n  \"benchmark\":\"fused-egress-construction\",\n  \"mode\":");
    push_json_string(&mut output, config.mode.as_str());
    output.push_str(",\n  \"hosted_ci_is_diagnostic_only\":true");
    output.push_str(",\n  \"byte_equivalence_required\":true");
    output.push_str(",\n  \"hardware\":");
    output.push_str(hardware_json);
    write!(
        output,
        ",\n  \"settings\":{{\"warmup_rounds\":{},\"measured_rounds\":{},\"iteration_scale\":{}}}",
        config.warmup_rounds, config.measured_rounds, config.iteration_scale
    )
    .expect("writing to String cannot fail");
    output.push_str(",\n  \"cases\":[");
    for (case_index, (case, body_len, iterations, moved_once, samples)) in reports.iter().enumerate() {
        if case_index != 0 {
            output.push(',');
        }
        let reference_summary = summarize(samples.iter().map(|sample| sample.reference_ns))?;
        let fused_summary = summarize(samples.iter().map(|sample| sample.fused_ns))?;
        write!(output, "{{\"name\":").expect("writing to String cannot fail");
        push_json_string(&mut output, case.name);
        write!(
            output,
            ",\"body_bytes\":{body_len},\"iterations_per_sample\":{iterations},\"reference_body_copy_bytes_per_packet\":{body_len},\"fused_finalize_move_bytes_per_packet\":{moved_once},\"reference\":{{\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{}}},\"fused\":{{\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{}}},\"paired_rounds\":[",
            reference_summary.p50_ns,
            reference_summary.p95_ns,
            reference_summary.p99_ns,
            reference_summary.max_ns,
            fused_summary.p50_ns,
            fused_summary.p95_ns,
            fused_summary.p99_ns,
            fused_summary.max_ns
        )
        .expect("writing to String cannot fail");
        for (sample_index, sample) in samples.iter().enumerate() {
            if sample_index != 0 {
                output.push(',');
            }
            write!(
                output,
                "{{\"round\":{},\"reference_first\":{},\"reference_ns\":{},\"fused_ns\":{},\"reference_checksum\":{},\"fused_checksum\":{}}}",
                sample.round,
                sample.reference_first,
                sample.reference_ns,
                sample.fused_ns,
                sample.reference_checksum,
                sample.fused_checksum
            )
            .expect("writing to String cannot fail");
        }
        output.push_str("]}");
    }
    output.push_str("]\n}\n");
    Ok(output)
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut args = args.into_iter().peekable();
    let mut mode = None;
    let mut output = None;
    let mut warmup_rounds = None;
    let mut measured_rounds = None;
    let mut iteration_scale = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--warmup-rounds" => {
                warmup_rounds = Some(parse_positive(
                    &next_value(&mut args, "--warmup-rounds")?,
                    "warmup rounds",
                )?);
            }
            "--measured-rounds" => {
                measured_rounds = Some(parse_positive(
                    &next_value(&mut args, "--measured-rounds")?,
                    "measured rounds",
                )?);
            }
            "--iteration-scale" => {
                iteration_scale = Some(parse_positive(
                    &next_value(&mut args, "--iteration-scale")?,
                    "iteration scale",
                )?);
            }
            "--help" | "-h" => {
                return Err("usage: egress_fusion_bench (--smoke|--full) [--output PATH] [--warmup-rounds N] [--measured-rounds N] [--iteration-scale N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    if let Some(value) = warmup_rounds {
        config.warmup_rounds = value;
    }
    if let Some(value) = measured_rounds {
        config.measured_rounds = value;
    }
    if let Some(value) = iteration_scale {
        config.iteration_scale = value;
    }
    Ok(config)
}

fn next_value(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    flag: &str,
) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_positive(value: &str, label: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|error| format!("invalid {label} {value:?}: {error}"))?;
    if parsed == 0 {
        return Err(format!("{label} must be positive"));
    }
    Ok(parsed)
}

fn set_mode(slot: &mut Option<Mode>, value: Mode) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err("exactly one benchmark mode is required".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BenchCase, FusedEgress, FusedError, MAX_UTF16_UNITS, Payload, build_fused, build_reference,
        deterministic_bytes, limits, write_payload,
    };
    use crucible_connection_core::EgressBuffer;
    use crucible_packet_core::{PacketCodecError, PacketWriter};

    fn raw_case(body_len: usize) -> BenchCase {
        assert!(body_len >= 1);
        BenchCase {
            name: "boundary",
            packet_id: 0,
            payload: Payload::Raw(vec![0x5a; body_len - 1]),
            base_iterations: 1,
        }
    }

    #[test]
    fn fused_matches_reference_across_frame_prefix_boundaries() {
        for body_len in [1, 2, 126, 127, 128, 129, 16_382, 16_383, 16_384, 16_385, 65_536] {
            let case = raw_case(body_len);
            let reference = build_reference(&case).expect("reference");
            let (fused, _) = build_fused(&case).expect("fused");
            assert_eq!(fused, reference, "body_len={body_len}");
        }
    }

    #[test]
    fn status_ping_and_mixed_shapes_match_reference_exactly() {
        let cases = [
            BenchCase {
                name: "ping",
                packet_id: 3,
                payload: Payload::Ping(-0x1122_3344_5566_778),
                base_iterations: 1,
            },
            BenchCase {
                name: "status",
                packet_id: 0,
                payload: Payload::Status("Crucible ⚒ status".repeat(4)),
                base_iterations: 1,
            },
            BenchCase {
                name: "mixed",
                packet_id: 127,
                payload: Payload::Metadata {
                    port: 25_565,
                    epoch: u64::MAX - 9,
                    enabled: true,
                    label: "semantic-equivalence".to_owned(),
                    bytes: deterministic_bytes(2_048, 0x1122_3344),
                },
                base_iterations: 1,
            },
        ];
        for case in &cases {
            assert_eq!(
                build_fused(case).expect("fused").0,
                build_reference(case).expect("reference"),
                "case={}",
                case.name
            );
        }
    }

    #[test]
    fn fused_failure_rolls_back_the_logical_queue_exactly() {
        let mut fused = FusedEgress::new(limits());
        fused
            .queue_packet(1, |writer| writer.write_i64(7))
            .expect("seed packet");
        let before = fused.pending().to_vec();
        let result = fused.queue_packet(2, |writer| {
            writer.write_bytes(b"partial")?;
            Err(PacketCodecError::InvalidBoolean(2))
        });
        assert_eq!(result, Err(FusedError::Packet(PacketCodecError::InvalidBoolean(2))));
        assert_eq!(fused.pending(), before);
    }

    #[test]
    fn oversized_direct_body_is_rejected_without_queue_mutation() {
        let mut fused = FusedEgress::new(limits());
        let before = fused.pending().to_vec();
        let oversized = vec![0_u8; limits().max_frame_body_len() + 1];
        assert!(fused.queue_packet(0, |writer| writer.write_bytes(&oversized)).is_err());
        assert_eq!(fused.pending(), before);
    }

    #[test]
    fn partial_drain_then_append_matches_reference_stream() {
        let first = BenchCase {
            name: "first",
            packet_id: 1,
            payload: Payload::Status("first".repeat(20)),
            base_iterations: 1,
        };
        let second = BenchCase {
            name: "second",
            packet_id: 2,
            payload: Payload::Raw(deterministic_bytes(4_096, 0xaabb_ccdd)),
            base_iterations: 1,
        };

        let mut reference = EgressBuffer::new(limits());
        let mut first_writer = PacketWriter::new(limits().max_frame_body_len()).expect("writer");
        first_writer.write_var_int(first.packet_id).expect("id");
        write_payload(&mut first_writer, &first.payload).expect("payload");
        reference.queue_frame(first_writer.as_slice()).expect("frame");

        let mut fused = FusedEgress::new(limits());
        fused
            .queue_packet(first.packet_id, |writer| write_payload(writer, &first.payload))
            .expect("fused first");
        assert_eq!(fused.pending(), reference.pending());

        let consumed = reference.queued_len() / 3;
        reference.consume_written(consumed).expect("reference drain");
        fused.consume_written(consumed).expect("fused drain");

        let mut second_writer = PacketWriter::new(limits().max_frame_body_len()).expect("writer");
        second_writer.write_var_int(second.packet_id).expect("id");
        write_payload(&mut second_writer, &second.payload).expect("payload");
        reference.queue_frame(second_writer.as_slice()).expect("frame");
        fused
            .queue_packet(second.packet_id, |writer| write_payload(writer, &second.payload))
            .expect("fused second");
        assert_eq!(fused.pending(), reference.pending());
    }

    #[test]
    fn string_limit_failure_is_transactional() {
        let mut fused = FusedEgress::new(limits());
        fused
            .queue_packet(1, |writer| writer.write_i64(1))
            .expect("seed");
        let before = fused.pending().to_vec();
        let too_long = "x".repeat(MAX_UTF16_UNITS + 1);
        assert!(
            fused
                .queue_packet(2, |writer| writer.write_string(&too_long, MAX_UTF16_UNITS))
                .is_err()
        );
        assert_eq!(fused.pending(), before);
    }

    #[test]
    fn small_frames_require_no_finalize_body_move() {
        let case = BenchCase {
            name: "small",
            packet_id: 1,
            payload: Payload::Ping(42),
            base_iterations: 1,
        };
        let (_, moved) = build_fused(&case).expect("fused");
        assert_eq!(moved, 0);
    }
}
