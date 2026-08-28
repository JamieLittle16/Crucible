use std::env;
use std::fmt::Write as _;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

use helve_benchmark_support::collect_hardware_metadata;
use helve_types::ChunkPos;
use helve_world_chunk::{RegionCellLayout, VerticalSectionLattice};

const SCHEMA: u32 = 1;
const REGION_SHIFT: u32 = 3;
const CHECKSUM_SEED: u64 = 0x9E37_79B9_7F4A_7C15;
const CHECKSUM_MUL: u64 = 0xD6E8_FEB8_6659_FD93;

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
    operations: usize,
    warmup_rounds: usize,
    measured_rounds: usize,
}

impl Config {
    fn defaults(mode: Mode) -> Self {
        match mode {
            Mode::Smoke => Self {
                mode,
                output: None,
                operations: 32_768,
                warmup_rounds: 2,
                measured_rounds: 10,
            },
            Mode::Full => Self {
                mode,
                output: None,
                operations: 1_048_576,
                warmup_rounds: 8,
                measured_rounds: 64,
            },
        }
    }
}

#[derive(Clone, Debug)]
struct PairEvidence {
    reference_ns: Vec<u128>,
    candidate_ns: Vec<u128>,
    checksum: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("spatial-address benchmark failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args(env::args().skip(1))?;
    let hardware = collect_hardware_metadata()?;
    let lattice = VerticalSectionLattice::new(-4, 24)
        .map_err(|error| format!("could not construct benchmark lattice: {error:?}"))?;
    let (ys, chunks) = prepare_traces(config.operations);
    validate_vertical(&lattice, &ys)?;
    validate_region(&chunks)?;

    let vertical = benchmark_pair(
        config.warmup_rounds,
        config.measured_rounds,
        || vertical_reference(&lattice, &ys),
        || vertical_candidate(&lattice, &ys),
    )?;
    let region = benchmark_pair(
        config.warmup_rounds,
        config.measured_rounds,
        || region_reference(&chunks),
        || region_candidate(&chunks),
    )?;

    let artifact = render_report(&config, &hardware.to_json(), &vertical, &region);
    if let Some(path) = config.output {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        fs::write(&path, artifact)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        eprintln!("wrote {}", path.display());
    } else {
        println!("{artifact}");
    }
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Config, String> {
    let mut args = args.into_iter();
    let mut mode = None;
    let mut output = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--output" => {
                output = Some(PathBuf::from(
                    args.next()
                        .ok_or_else(|| "--output requires a path".to_owned())?,
                ));
            }
            "--help" | "-h" => {
                return Err(
                    "usage: spatial_address_bench (--smoke|--full) [--output PATH]".to_owned(),
                );
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let mode = mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?;
    let mut config = Config::defaults(mode);
    config.output = output;
    Ok(config)
}

fn set_mode(slot: &mut Option<Mode>, value: Mode) -> Result<(), String> {
    if slot.replace(value).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
}

fn prepare_traces(operations: usize) -> (Vec<i32>, Vec<ChunkPos>) {
    let mut ys = Vec::with_capacity(operations);
    let mut chunks = Vec::with_capacity(operations);
    let mut state = 0xA341_316C_D4F2_9B17_u64;
    for index in 0..operations {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let y = i32::try_from(state % 448).expect("bounded y") - 96;
        ys.push(y);

        state = state.rotate_left(19) ^ u64::try_from(index).expect("index fits u64");
        let x = i32::from_ne_bytes(
            u32::try_from(state & u64::from(u32::MAX))
                .expect("masked x")
                .to_ne_bytes(),
        );
        state = state.rotate_left(23) ^ 0x517C_C1B7_2722_0A95;
        let z = i32::from_ne_bytes(
            u32::try_from(state & u64::from(u32::MAX))
                .expect("masked z")
                .to_ne_bytes(),
        );
        chunks.push(ChunkPos { x, z });
    }
    (ys, chunks)
}

fn validate_vertical(lattice: &VerticalSectionLattice, ys: &[i32]) -> Result<(), String> {
    for &y in ys {
        let reference = vertical_reference_one(lattice, y);
        let candidate = vertical_candidate_one(lattice, y);
        if reference != candidate {
            return Err(format!(
                "vertical semantic mismatch at y={y}: reference={reference:?} candidate={candidate:?}"
            ));
        }
    }
    Ok(())
}

fn validate_region(chunks: &[ChunkPos]) -> Result<(), String> {
    for &chunk in chunks {
        let reference = region_reference_one(chunk);
        let candidate = RegionCellLayout::<REGION_SHIFT>::address(chunk);
        let observed = (
            candidate.cell().x,
            candidate.cell().z,
            candidate.local_x(),
            candidate.local_z(),
            candidate.slot(),
        );
        if reference != observed {
            return Err(format!(
                "region semantic mismatch at {chunk:?}: reference={reference:?} candidate={observed:?}"
            ));
        }
    }
    Ok(())
}

fn vertical_reference_one(lattice: &VerticalSectionLattice, y: i32) -> Option<(usize, u8)> {
    let section_y = y.div_euclid(16);
    let offset = i64::from(section_y) - i64::from(lattice.min_section_y());
    let index = usize::try_from(offset).ok()?;
    if index >= lattice.section_count() {
        return None;
    }
    let local = u8::try_from(y.rem_euclid(16)).expect("Euclidean local Y");
    Some((index, local))
}

fn vertical_candidate_one(lattice: &VerticalSectionLattice, y: i32) -> Option<(usize, u8)> {
    lattice.resolve_block_y(y)
}

fn vertical_reference(lattice: &VerticalSectionLattice, ys: &[i32]) -> u64 {
    let mut checksum = CHECKSUM_SEED;
    for &y in ys {
        let encoded = vertical_reference_one(lattice, y).map_or(u64::MAX, |(index, local)| {
            u64::try_from(index).expect("section index fits u64") << 8 | u64::from(local)
        });
        checksum = checksum.rotate_left(9) ^ encoded.wrapping_mul(CHECKSUM_MUL);
    }
    black_box(checksum)
}

fn vertical_candidate(lattice: &VerticalSectionLattice, ys: &[i32]) -> u64 {
    let mut checksum = CHECKSUM_SEED;
    for &y in ys {
        let encoded = vertical_candidate_one(lattice, y).map_or(u64::MAX, |(index, local)| {
            u64::try_from(index).expect("section index fits u64") << 8 | u64::from(local)
        });
        checksum = checksum.rotate_left(9) ^ encoded.wrapping_mul(CHECKSUM_MUL);
    }
    black_box(checksum)
}

fn region_reference_one(chunk: ChunkPos) -> (i32, i32, u16, u16, u32) {
    let side = i32::try_from(RegionCellLayout::<REGION_SHIFT>::side_chunks())
        .expect("benchmark side fits i32");
    let cell_x = chunk.x.div_euclid(side);
    let cell_z = chunk.z.div_euclid(side);
    let local_x = u16::try_from(chunk.x.rem_euclid(side)).expect("reference local x");
    let local_z = u16::try_from(chunk.z.rem_euclid(side)).expect("reference local z");
    let slot =
        u32::from(local_z) * u32::try_from(side).expect("positive side") + u32::from(local_x);
    (cell_x, cell_z, local_x, local_z, slot)
}

fn region_reference(chunks: &[ChunkPos]) -> u64 {
    let mut checksum = CHECKSUM_SEED;
    for &chunk in chunks {
        let (cell_x, cell_z, local_x, local_z, slot) = region_reference_one(chunk);
        checksum = mix_region(checksum, cell_x, cell_z, local_x, local_z, slot);
    }
    black_box(checksum)
}

fn region_candidate(chunks: &[ChunkPos]) -> u64 {
    let mut checksum = CHECKSUM_SEED;
    for &chunk in chunks {
        let address = RegionCellLayout::<REGION_SHIFT>::address(chunk);
        checksum = mix_region(
            checksum,
            address.cell().x,
            address.cell().z,
            address.local_x(),
            address.local_z(),
            address.slot(),
        );
    }
    black_box(checksum)
}

fn mix_region(
    checksum: u64,
    cell_x: i32,
    cell_z: i32,
    local_x: u16,
    local_z: u16,
    slot: u32,
) -> u64 {
    let x = u64::from(u32::from_ne_bytes(cell_x.to_ne_bytes()));
    let z = u64::from(u32::from_ne_bytes(cell_z.to_ne_bytes()));
    let local = (u64::from(local_z) << 16) | u64::from(local_x);
    checksum.rotate_left(11)
        ^ x.wrapping_mul(CHECKSUM_MUL)
        ^ z.rotate_left(17)
        ^ local.rotate_left(31)
        ^ u64::from(slot)
}

fn benchmark_pair<R, C>(
    warmup_rounds: usize,
    measured_rounds: usize,
    mut reference: R,
    mut candidate: C,
) -> Result<PairEvidence, String>
where
    R: FnMut() -> u64,
    C: FnMut() -> u64,
{
    let reference_checksum = reference();
    let candidate_checksum = candidate();
    if reference_checksum != candidate_checksum {
        return Err(format!(
            "benchmark semantic checksum mismatch: reference={reference_checksum} candidate={candidate_checksum}"
        ));
    }

    for round in 0..warmup_rounds {
        if round % 2 == 0 {
            black_box(reference());
            black_box(candidate());
        } else {
            black_box(candidate());
            black_box(reference());
        }
    }

    let mut reference_ns = Vec::with_capacity(measured_rounds);
    let mut candidate_ns = Vec::with_capacity(measured_rounds);
    for round in 0..measured_rounds {
        if round % 2 == 0 {
            reference_ns.push(time_once(&mut reference));
            candidate_ns.push(time_once(&mut candidate));
        } else {
            candidate_ns.push(time_once(&mut candidate));
            reference_ns.push(time_once(&mut reference));
        }
    }
    Ok(PairEvidence {
        reference_ns,
        candidate_ns,
        checksum: reference_checksum,
    })
}

fn time_once<F: FnMut() -> u64>(work: &mut F) -> u128 {
    let start = Instant::now();
    black_box(work());
    start.elapsed().as_nanos()
}

fn render_report(
    config: &Config,
    hardware_json: &str,
    vertical: &PairEvidence,
    region: &PairEvidence,
) -> String {
    let mut output = String::new();
    write!(
        output,
        "{{\"schema\":{SCHEMA},\"benchmark\":\"spatial-address-primitives\",\"mode\":\"{}\",\"hosted_ci_is_diagnostic_only\":true,\"operations\":{},\"warmup_rounds\":{},\"measured_rounds\":{},\"region_shift\":{REGION_SHIFT},\"hardware\":{hardware_json},",
        config.mode.as_str(),
        config.operations,
        config.warmup_rounds,
        config.measured_rounds,
    )
    .expect("writing to String cannot fail");
    push_pair_json(&mut output, "vertical", vertical);
    output.push(',');
    push_pair_json(&mut output, "region", region);
    output.push('}');
    output
}

fn push_pair_json(output: &mut String, name: &str, evidence: &PairEvidence) {
    write!(
        output,
        "\"{name}\":{{\"semantic_equivalent\":true,\"checksum\":{},\"reference_ns\":",
        evidence.checksum
    )
    .expect("writing to String cannot fail");
    push_u128_array(output, &evidence.reference_ns);
    output.push_str(",\"candidate_ns\":");
    push_u128_array(output, &evidence.candidate_ns);
    output.push('}');
}

fn push_u128_array(output: &mut String, values: &[u128]) {
    output.push('[');
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        write!(output, "{value}").expect("writing to String cannot fail");
    }
    output.push(']');
}
