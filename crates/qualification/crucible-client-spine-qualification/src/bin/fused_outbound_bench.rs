use std::env;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use crucible_benchmark_support::{collect_hardware_metadata, push_json_string};
use crucible_connection_core::{ConnectionLimits, EgressBuffer};
use crucible_packet_core::PacketWriter;
use crucible_protocol_core::{encode_string, encode_var_int, var_int_len};

const SCHEMA: u32 = 1;
const MAX_BODY: usize = 65_536;
const EGRESS_LIMIT: usize = 2 * 1024 * 1024;
const CHECKSUM_SEED: u64 = 0xA076_1D64_78BD_642F;
const CHECKSUM_MUL: u64 = 0xE703_7ED1_A0B4_28DB;

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
    base_operations: usize,
    warmup_rounds: usize,
    measured_rounds: usize,
}

impl Config {
    const fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                base_operations: 256,
                warmup_rounds: 1,
                measured_rounds: 6,
            },
            Mode::Full => Self {
                mode,
                output: None,
                base_operations: 4_096,
                warmup_rounds: 6,
                measured_rounds: 32,
            },
        }
    }
}

#[derive(Clone, Debug)]
enum PacketShape {
    Ping {
        packet_id: i32,
        payload: i64,
    },
    Status {
        packet_id: i32,
        text: &'static str,
        max_utf16_units: usize,
    },
    Metadata {
        packet_id: i32,
        sequence: i32,
        port: u16,
        enabled: bool,
        name: &'static str,
        max_name_utf16_units: usize,
    },
    Blob {
        packet_id: i32,
        bytes: Vec<u8>,
    },
}

impl PacketShape {
    const fn packet_id(&self) -> i32 {
        match self {
            Self::Ping { packet_id, .. }
            | Self::Status { packet_id, .. }
            | Self::Metadata { packet_id, .. }
            | Self::Blob { packet_id, .. } => *packet_id,
        }
    }

    fn body_len(&self) -> Result<usize, String> {
        let payload_len = match self {
            Self::Ping { .. } => 8,
            Self::Status {
                text,
                max_utf16_units,
                ..
            } => string_wire_len(text, *max_utf16_units)?,
            Self::Metadata {
                sequence,
                name,
                max_name_utf16_units,
                ..
            } => {
                let name_len = string_wire_len(name, *max_name_utf16_units)?;
                var_int_len(*sequence)
                    .checked_add(3)
                    .and_then(|value| value.checked_add(name_len))
                    .ok_or_else(|| "metadata body length overflow".to_owned())?
            }
            Self::Blob { bytes, .. } => bytes.len(),
        };
        var_int_len(self.packet_id())
            .checked_add(payload_len)
            .ok_or_else(|| "packet body length overflow".to_owned())
    }

    fn write_reference(&self, writer: &mut PacketWriter) -> Result<(), String> {
        writer
            .write_var_int(self.packet_id())
            .map_err(|error| format!("packet id rejected: {error:?}"))?;
        match self {
            Self::Ping { payload, .. } => writer
                .write_i64(*payload)
                .map_err(|error| format!("ping payload rejected: {error:?}"))?,
            Self::Status {
                text,
                max_utf16_units,
                ..
            } => writer
                .write_string(text, *max_utf16_units)
                .map_err(|error| format!("status string rejected: {error:?}"))?,
            Self::Metadata {
                sequence,
                port,
                enabled,
                name,
                max_name_utf16_units,
                ..
            } => {
                writer
                    .write_var_int(*sequence)
                    .map_err(|error| format!("metadata sequence rejected: {error:?}"))?;
                writer
                    .write_u16(*port)
                    .map_err(|error| format!("metadata port rejected: {error:?}"))?;
                writer
                    .write_bool(*enabled)
                    .map_err(|error| format!("metadata flag rejected: {error:?}"))?;
                writer
                    .write_string(name, *max_name_utf16_units)
                    .map_err(|error| format!("metadata name rejected: {error:?}"))?;
            }
            Self::Blob { bytes, .. } => writer
                .write_bytes(bytes)
                .map_err(|error| format!("blob rejected: {error:?}"))?,
        }
        Ok(())
    }

    fn write_fused(&self, output: &mut Vec<u8>) -> Result<(), String> {
        encode_var_int(self.packet_id(), output);
        match self {
            Self::Ping { payload, .. } => output.extend_from_slice(&payload.to_be_bytes()),
            Self::Status {
                text,
                max_utf16_units,
                ..
            } => encode_string(text, *max_utf16_units, output)
                .map_err(|error| format!("status string rejected: {error:?}"))?,
            Self::Metadata {
                sequence,
                port,
                enabled,
                name,
                max_name_utf16_units,
                ..
            } => {
                encode_var_int(*sequence, output);
                output.extend_from_slice(&port.to_be_bytes());
                output.push(u8::from(*enabled));
                encode_string(name, *max_name_utf16_units, output)
                    .map_err(|error| format!("metadata name rejected: {error:?}"))?;
            }
            Self::Blob { bytes, .. } => output.extend_from_slice(bytes),
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct TraceCase {
    name: &'static str,
    shapes: Vec<PacketShape>,
    operation_divisor: usize,
}

impl TraceCase {
    fn operations(&self, base: usize) -> usize {
        (base / self.operation_divisor).max(1)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TraceResult {
    checksum: u64,
    emitted_body_bytes: u64,
    final_queued_bytes: usize,
}

#[derive(Clone, Copy, Debug)]
struct PairSample {
    round: usize,
    fused_first: bool,
    reference_ns: u128,
    fused_ns: u128,
}

#[derive(Clone, Copy, Debug)]
struct NanosecondSummary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

#[derive(Debug)]
struct CaseEvidence {
    name: &'static str,
    operations: usize,
    semantic: TraceResult,
    reference: NanosecondSummary,
    fused: NanosecondSummary,
    samples: Vec<PairSample>,
}

#[derive(Debug)]
struct FusedEgress {
    bytes: Vec<u8>,
    start: usize,
    max_body: usize,
    max_queued: usize,
}

impl FusedEgress {
    const fn new(max_body: usize, max_queued: usize) -> Self {
        Self {
            bytes: Vec::new(),
            start: 0,
            max_body,
            max_queued,
        }
    }

    fn pending(&self) -> &[u8] {
        &self.bytes[self.start..]
    }

    fn queued_len(&self) -> usize {
        self.bytes.len() - self.start
    }

    fn queue_shape(&mut self, shape: &PacketShape) -> Result<(), String> {
        let body_len = shape.body_len()?;
        if body_len == 0 {
            return Err("qualification candidate refuses zero-length frame body".to_owned());
        }
        if body_len > self.max_body {
            return Err(format!(
                "candidate body limit exceeded: body={body_len}, maximum={}",
                self.max_body
            ));
        }
        let signed_len = i32::try_from(body_len)
            .map_err(|_| format!("candidate body length does not fit VarInt: {body_len}"))?;
        let frame_len = var_int_len(signed_len)
            .checked_add(body_len)
            .ok_or_else(|| "candidate frame length overflow".to_owned())?;
        let required = self
            .queued_len()
            .checked_add(frame_len)
            .ok_or_else(|| "candidate egress length overflow".to_owned())?;
        if required > self.max_queued {
            return Err(format!(
                "candidate egress limit exceeded: queued={}, frame={frame_len}, maximum={}",
                self.queued_len(),
                self.max_queued
            ));
        }

        self.compact_before_append(frame_len)?;
        let append_start = self.bytes.len();
        self.bytes.reserve(frame_len);
        encode_var_int(signed_len, &mut self.bytes);
        if let Err(error) = shape.write_fused(&mut self.bytes) {
            self.bytes.truncate(append_start);
            return Err(error);
        }
        let appended = self.bytes.len() - append_start;
        if appended != frame_len {
            self.bytes.truncate(append_start);
            return Err(format!(
                "candidate encoded length mismatch: expected={frame_len}, actual={appended}"
            ));
        }
        Ok(())
    }

    fn consume_written(&mut self, written: usize) -> Result<(), String> {
        let available = self.queued_len();
        if written > available {
            return Err(format!(
                "candidate consumed beyond queue: requested={written}, available={available}"
            ));
        }
        self.start += written;
        if self.start == self.bytes.len() {
            self.bytes.clear();
            self.start = 0;
        }
        Ok(())
    }

    fn compact_before_append(&mut self, incoming: usize) -> Result<(), String> {
        if self.start == 0 {
            return Ok(());
        }
        let physical_after = self
            .bytes
            .len()
            .checked_add(incoming)
            .ok_or_else(|| "candidate physical egress length overflow".to_owned())?;
        let half_consumed = self.start >= self.bytes.len() / 2;
        if physical_after > self.max_queued || half_consumed {
            let active = self.queued_len();
            self.bytes.copy_within(self.start.., 0);
            self.bytes.truncate(active);
            self.start = 0;
        }
        Ok(())
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fused outbound benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    let cases = trace_cases()?;
    let mut evidence = Vec::with_capacity(cases.len());

    for case in &cases {
        let operations = case.operations(config.base_operations);
        let semantic = require_byte_equivalence(case, operations)?;

        for round in 0..config.warmup_rounds {
            let fused_first = !round.is_multiple_of(2);
            black_box(measure_pair(case, operations, fused_first)?);
        }

        let mut samples = Vec::with_capacity(config.measured_rounds);
        for round in 0..config.measured_rounds {
            let fused_first = !round.is_multiple_of(2);
            let (reference_ns, fused_ns) = measure_pair(case, operations, fused_first)?;
            samples.push(PairSample {
                round,
                fused_first,
                reference_ns,
                fused_ns,
            });
        }
        let reference = summarize(samples.iter().map(|sample| sample.reference_ns).collect());
        let fused = summarize(samples.iter().map(|sample| sample.fused_ns).collect());
        evidence.push(CaseEvidence {
            name: case.name,
            operations,
            semantic,
            reference,
            fused,
            samples,
        });
    }

    let report = render_report(&config, &hardware.to_json(), &evidence);
    if let Some(path) = config.output {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
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

fn trace_cases() -> Result<Vec<TraceCase>, String> {
    let large = pattern_bytes(32_768, 251)?;
    let medium = pattern_bytes(4_096, 239)?;
    Ok(vec![
        TraceCase {
            name: "tiny_ping",
            shapes: vec![PacketShape::Ping {
                packet_id: 1,
                payload: 0x0102_0304_0506_0708,
            }],
            operation_divisor: 1,
        },
        TraceCase {
            name: "status_string",
            shapes: vec![PacketShape::Status {
                packet_id: 2,
                text: "Crucible qualification status payload — deterministic and bounded",
                max_utf16_units: 128,
            }],
            operation_divisor: 1,
        },
        TraceCase {
            name: "medium_metadata",
            shapes: vec![PacketShape::Metadata {
                packet_id: 3,
                sequence: 131_071,
                port: 25_565,
                enabled: true,
                name: "qualification-metadata",
                max_name_utf16_units: 64,
            }],
            operation_divisor: 1,
        },
        TraceCase {
            name: "large_blob",
            shapes: vec![PacketShape::Blob {
                packet_id: 4,
                bytes: large,
            }],
            operation_divisor: 16,
        },
        TraceCase {
            name: "coalesced_mix",
            shapes: vec![
                PacketShape::Ping {
                    packet_id: 5,
                    payload: -7,
                },
                PacketShape::Status {
                    packet_id: 6,
                    text: "mixed-status",
                    max_utf16_units: 32,
                },
                PacketShape::Metadata {
                    packet_id: 7,
                    sequence: 128,
                    port: 25_565,
                    enabled: false,
                    name: "mixed-metadata",
                    max_name_utf16_units: 32,
                },
                PacketShape::Blob {
                    packet_id: 8,
                    bytes: medium,
                },
            ],
            operation_divisor: 2,
        },
    ])
}

fn pattern_bytes(len: usize, modulus: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(len);
    for index in 0..len {
        let value = u8::try_from(index % modulus)
            .map_err(|_| "qualification pattern value does not fit u8".to_owned())?;
        bytes.push(value);
    }
    Ok(bytes)
}

fn require_byte_equivalence(case: &TraceCase, operations: usize) -> Result<TraceResult, String> {
    let limits = limits()?;
    let mut reference = EgressBuffer::new(limits);
    let mut fused = FusedEgress::new(MAX_BODY, EGRESS_LIMIT);
    let mut emitted_body_bytes = 0_u64;

    for operation in 0..operations {
        let shape = &case.shapes[operation % case.shapes.len()];
        emitted_body_bytes = add_body_bytes(emitted_body_bytes, shape)?;
        queue_reference(&mut reference, shape)?;
        fused.queue_shape(shape)?;
        require_same_pending(case.name, operation, "queue", reference.pending(), fused.pending())?;

        let drain = drain_amount(reference.pending().len(), operation);
        if drain != 0 {
            reference
                .consume_written(drain)
                .map_err(|error| format!("reference drain failed: {error:?}"))?;
            fused.consume_written(drain)?;
            require_same_pending(
                case.name,
                operation,
                "drain",
                reference.pending(),
                fused.pending(),
            )?;
        }
    }
    Ok(trace_result(reference.pending(), emitted_body_bytes))
}

fn require_same_pending(
    case: &str,
    operation: usize,
    phase: &str,
    reference: &[u8],
    fused: &[u8],
) -> Result<(), String> {
    if reference == fused {
        return Ok(());
    }
    Err(format!(
        "byte divergence in {case} after {phase} at operation {operation}"
    ))
}

fn measure_pair(
    case: &TraceCase,
    operations: usize,
    fused_first: bool,
) -> Result<(u128, u128), String> {
    let (reference, fused) = if fused_first {
        let fused = measure_fused(case, operations)?;
        let reference = measure_reference(case, operations)?;
        (reference, fused)
    } else {
        let reference = measure_reference(case, operations)?;
        let fused = measure_fused(case, operations)?;
        (reference, fused)
    };
    if reference.1 != fused.1 {
        return Err(format!("timed semantic divergence in {}", case.name));
    }
    Ok((reference.0, fused.0))
}

fn measure_reference(case: &TraceCase, operations: usize) -> Result<(u128, TraceResult), String> {
    let limits = limits()?;
    let start = Instant::now();
    let mut queue = EgressBuffer::new(limits);
    let mut emitted_body_bytes = 0_u64;
    for operation in 0..operations {
        let shape = black_box(&case.shapes[operation % case.shapes.len()]);
        emitted_body_bytes = add_body_bytes(emitted_body_bytes, shape)?;
        queue_reference(&mut queue, shape)?;
        let drain = drain_amount(queue.pending().len(), operation);
        if drain != 0 {
            queue
                .consume_written(drain)
                .map_err(|error| format!("reference drain failed: {error:?}"))?;
        }
    }
    let result = trace_result(black_box(queue.pending()), emitted_body_bytes);
    Ok((start.elapsed().as_nanos(), black_box(result)))
}

fn measure_fused(case: &TraceCase, operations: usize) -> Result<(u128, TraceResult), String> {
    let start = Instant::now();
    let mut queue = FusedEgress::new(MAX_BODY, EGRESS_LIMIT);
    let mut emitted_body_bytes = 0_u64;
    for operation in 0..operations {
        let shape = black_box(&case.shapes[operation % case.shapes.len()]);
        emitted_body_bytes = add_body_bytes(emitted_body_bytes, shape)?;
        queue.queue_shape(shape)?;
        let drain = drain_amount(queue.pending().len(), operation);
        if drain != 0 {
            queue.consume_written(drain)?;
        }
    }
    let result = trace_result(black_box(queue.pending()), emitted_body_bytes);
    Ok((start.elapsed().as_nanos(), black_box(result)))
}

fn add_body_bytes(current: u64, shape: &PacketShape) -> Result<u64, String> {
    let body_len = u64::try_from(shape.body_len()?)
        .map_err(|_| "body length does not fit u64".to_owned())?;
    current
        .checked_add(body_len)
        .ok_or_else(|| "emitted body byte count overflow".to_owned())
}

fn trace_result(pending: &[u8], emitted_body_bytes: u64) -> TraceResult {
    TraceResult {
        checksum: checksum_bytes(pending) ^ emitted_body_bytes,
        emitted_body_bytes,
        final_queued_bytes: pending.len(),
    }
}

fn queue_reference(queue: &mut EgressBuffer, shape: &PacketShape) -> Result<(), String> {
    let mut writer = PacketWriter::new(MAX_BODY)
        .map_err(|error| format!("reference writer construction failed: {error:?}"))?;
    shape.write_reference(&mut writer)?;
    queue
        .queue_frame(writer.as_slice())
        .map_err(|error| format!("reference frame queue rejected: {error:?}"))
}

fn drain_amount(queued: usize, operation: usize) -> usize {
    let sequence = operation + 1;
    if sequence.is_multiple_of(23) {
        queued
    } else if sequence.is_multiple_of(5) {
        queued / 3
    } else {
        0
    }
}

fn limits() -> Result<ConnectionLimits, String> {
    ConnectionLimits::new(MAX_BODY, EGRESS_LIMIT, EGRESS_LIMIT)
        .map_err(|error| format!("invalid qualification limits: {error:?}"))
}

fn string_wire_len(value: &str, max_utf16_units: usize) -> Result<usize, String> {
    let utf16_units = value.encode_utf16().count();
    if utf16_units > max_utf16_units {
        return Err(format!(
            "string UTF-16 limit exceeded: units={utf16_units}, maximum={max_utf16_units}"
        ));
    }
    let max_bytes = max_utf16_units
        .checked_mul(3)
        .ok_or_else(|| "string byte limit overflow".to_owned())?;
    if value.len() > max_bytes {
        return Err(format!(
            "string byte limit exceeded: bytes={}, maximum={max_bytes}",
            value.len()
        ));
    }
    let signed_len = i32::try_from(value.len())
        .map_err(|_| format!("string byte length does not fit VarInt: {}", value.len()))?;
    var_int_len(signed_len)
        .checked_add(value.len())
        .ok_or_else(|| "string encoded length overflow".to_owned())
}

fn checksum_bytes(bytes: &[u8]) -> u64 {
    let mut checksum = CHECKSUM_SEED;
    for &byte in bytes {
        checksum ^= u64::from(byte).wrapping_add(CHECKSUM_SEED);
        checksum = checksum.rotate_left(17).wrapping_mul(CHECKSUM_MUL);
    }
    checksum
}

fn summarize(mut values: Vec<u128>) -> NanosecondSummary {
    values.sort_unstable();
    NanosecondSummary {
        p50: percentile(&values, 50),
        p95: percentile(&values, 95),
        p99: percentile(&values, 99),
        max: values.last().copied().unwrap_or(0),
    }
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) * percentile).div_ceil(100);
    sorted[index]
}

fn ratio_millionths(numerator: u128, denominator: u128) -> u128 {
    if denominator == 0 {
        return 0;
    }
    numerator
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_div(denominator))
        .unwrap_or(0)
}

fn render_report(config: &Config, hardware_json: &str, cases: &[CaseEvidence]) -> String {
    let mut output = String::new();
    write!(output, "{{\n  \"schema\":{SCHEMA}").expect("writing to String cannot fail");
    output.push_str(",\n  \"benchmark\":\"fused-outbound-construction\"");
    output.push_str(",\n  \"mode\":");
    push_json_string(&mut output, config.mode.as_str());
    output.push_str(",\n  \"hosted_ci_is_diagnostic_only\":true");
    output.push_str(",\n  \"production_path_unchanged\":true");
    output.push_str(",\n  \"reference_intermediate_body_vec\":true");
    output.push_str(",\n  \"candidate_direct_final_buffer\":true");
    output.push_str(",\n  \"hardware\":");
    output.push_str(hardware_json);
    output.push_str(",\n  \"cases\":[");

    for (case_index, case) in cases.iter().enumerate() {
        if case_index != 0 {
            output.push(',');
        }
        let fused_over_reference = ratio_millionths(case.fused.p50, case.reference.p50);
        write!(output, "{{\"name\":").expect("writing to String cannot fail");
        push_json_string(&mut output, case.name);
        write!(
            output,
            ",\"operations\":{},\"byte_equivalent\":true,\"emitted_body_bytes\":{},\"final_queued_bytes\":{},\"semantic_checksum\":{},\"reference\":{{\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{}}},\"fused\":{{\"p50_ns\":{},\"p95_ns\":{},\"p99_ns\":{},\"max_ns\":{}}},\"fused_over_reference_millionths\":{},\"paired_rounds\":[",
            case.operations,
            case.semantic.emitted_body_bytes,
            case.semantic.final_queued_bytes,
            case.semantic.checksum,
            case.reference.p50,
            case.reference.p95,
            case.reference.p99,
            case.reference.max,
            case.fused.p50,
            case.fused.p95,
            case.fused.p99,
            case.fused.max,
            fused_over_reference
        )
        .expect("writing to String cannot fail");
        for (sample_index, sample) in case.samples.iter().enumerate() {
            if sample_index != 0 {
                output.push(',');
            }
            write!(
                output,
                "{{\"round\":{},\"fused_first\":{},\"reference_ns\":{},\"fused_ns\":{}}}",
                sample.round, sample.fused_first, sample.reference_ns, sample.fused_ns
            )
            .expect("writing to String cannot fail");
        }
        output.push_str("]}");
    }
    output.push_str("]\n}\n");
    output
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut args = args.into_iter().peekable();
    let mut mode = None;
    let mut output = None;
    let mut operations = None;
    let mut warmup_rounds = None;
    let mut measured_rounds = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--output" => output = Some(PathBuf::from(next_value(&mut args, "--output")?)),
            "--operations" => {
                operations = Some(parse_positive(
                    &next_value(&mut args, "--operations")?,
                    "operations",
                )?);
            }
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
            "--help" | "-h" => {
                return Err("usage: fused_outbound_bench (--smoke|--full) [--output PATH] [--operations N] [--warmup-rounds N] [--measured-rounds N]".to_owned());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    if let Some(value) = operations {
        config.base_operations = value;
    }
    if let Some(value) = warmup_rounds {
        config.warmup_rounds = value;
    }
    if let Some(value) = measured_rounds {
        config.measured_rounds = value;
    }
    Ok(config)
}

fn set_mode(slot: &mut Option<Mode>, value: Mode) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
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
        .map_err(|_| format!("invalid {label}: {value}"))?;
    if parsed == 0 {
        return Err(format!("{label} must be positive"));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::{
        EGRESS_LIMIT, FusedEgress, MAX_BODY, PacketShape, TraceCase, limits, queue_reference,
        require_byte_equivalence,
    };
    use crucible_connection_core::EgressBuffer;

    #[test]
    fn frame_prefix_boundaries_match_reference_exactly() {
        for body_len in [127_usize, 128, 16_383, 16_384] {
            let shape = PacketShape::Blob {
                packet_id: 1,
                bytes: vec![0x5a; body_len - 1],
            };
            let mut reference = EgressBuffer::new(limits().expect("limits"));
            queue_reference(&mut reference, &shape).expect("reference frame");
            let mut fused = FusedEgress::new(MAX_BODY, EGRESS_LIMIT);
            fused.queue_shape(&shape).expect("fused frame");
            assert_eq!(fused.pending(), reference.pending(), "body_len={body_len}");
        }
    }

    #[test]
    fn repeated_partial_drains_remain_byte_identical() {
        let case = TraceCase {
            name: "test-mixed",
            shapes: vec![
                PacketShape::Ping {
                    packet_id: 1,
                    payload: -42,
                },
                PacketShape::Status {
                    packet_id: 2,
                    text: "partial-drain",
                    max_utf16_units: 32,
                },
                PacketShape::Blob {
                    packet_id: 3,
                    bytes: vec![0x77; 4_096],
                },
            ],
            operation_divisor: 1,
        };
        let result = require_byte_equivalence(&case, 10_000).expect("trace must match");
        assert_ne!(result.checksum, 0);
        assert!(result.emitted_body_bytes > 0);
    }

    #[test]
    fn candidate_failure_rolls_back_existing_bytes() {
        let existing = PacketShape::Ping {
            packet_id: 1,
            payload: 7,
        };
        let oversized = PacketShape::Blob {
            packet_id: 2,
            bytes: vec![0xaa; 64],
        };
        let mut fused = FusedEgress::new(16, 256);
        fused.queue_shape(&existing).expect("existing frame");
        let before = fused.pending().to_vec();
        assert!(fused.queue_shape(&oversized).is_err());
        assert_eq!(fused.pending(), before);
    }

    #[test]
    fn invalid_string_rolls_back_final_buffer() {
        let existing = PacketShape::Ping {
            packet_id: 1,
            payload: 11,
        };
        let invalid = PacketShape::Status {
            packet_id: 2,
            text: "abcdef",
            max_utf16_units: 2,
        };
        let mut fused = FusedEgress::new(MAX_BODY, EGRESS_LIMIT);
        fused.queue_shape(&existing).expect("existing frame");
        let before = fused.pending().to_vec();
        assert!(fused.queue_shape(&invalid).is_err());
        assert_eq!(fused.pending(), before);
    }

    #[test]
    fn candidate_rejects_egress_overflow_without_mutation() {
        let shape = PacketShape::Blob {
            packet_id: 1,
            bytes: vec![0x33; 32],
        };
        let mut fused = FusedEgress::new(64, 40);
        fused.queue_shape(&shape).expect("first frame fits");
        let before = fused.pending().to_vec();
        assert!(fused.queue_shape(&shape).is_err());
        assert_eq!(fused.pending(), before);
    }
}
