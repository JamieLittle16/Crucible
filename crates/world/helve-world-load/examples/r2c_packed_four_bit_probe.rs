use std::{env, hint::black_box, time::Instant};

use helve_benchmark_support::collect_hardware_metadata;

const CELLS: usize = 4096;
const FOUR_BITS: usize = 4;
const FOUR_VALUES_PER_WORD: usize = 16;
const WORDS: usize = CELLS / FOUR_VALUES_PER_WORD;
const FOUR_MASK: u64 = 0x0f;
const PALETTE: [u16; 2] = [0, 1];
const ERROR_CELL: usize = 37;
const ERROR_INDEX: usize = 3;

#[derive(Clone, Copy)]
enum Mode {
    Smoke,
    Full,
}

impl Mode {
    const fn rounds(self) -> usize {
        match self {
            Self::Smoke => 2048,
            Self::Full => 32_768,
        }
    }

    const fn warmups(self) -> usize {
        match self {
            Self::Smoke => 256,
            Self::Full => 2048,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Smoke => "smoke",
            Self::Full => "full",
        }
    }
}

struct Config {
    mode: Mode,
    require_single_cpu: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProbeError {
    ArithmeticOverflow,
    PaletteIndexOutOfRange {
        cell: usize,
        palette_index: usize,
        palette_entries: usize,
    },
}

#[derive(Clone, Copy)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("r2c packed four-bit specialization probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let hardware = collect_hardware_metadata()?;
    if config.require_single_cpu && hardware.single_allowed_cpu().is_none() {
        return Err(format!(
            "--require-single-cpu requested but affinity is {}",
            hardware.cpus_allowed_list
        ));
    }

    let words = fixture_words();
    validate_error_equivalence(words)?;

    let mut generic_out = Vec::with_capacity(CELLS);
    let mut specialized_out = Vec::with_capacity(CELLS);
    decode_runtime_generic(&words, &PALETTE, black_box(FOUR_BITS), &mut generic_out)
        .map_err(|error| format!("generic validation decode failed: {error:?}"))?;
    decode_four_bit_specialized(&words, &PALETTE, &mut specialized_out)
        .map_err(|error| format!("specialized validation decode failed: {error:?}"))?;
    if generic_out != specialized_out {
        return Err("four-bit specialization changed packed semantics".to_owned());
    }
    let expected_checksum = checksum(&generic_out);

    for round in 0..config.mode.warmups() {
        black_box(run_once(
            &words,
            &mut generic_out,
            &mut specialized_out,
            round % 2 == 0,
        )?);
    }

    let mut generic_ns = Vec::with_capacity(config.mode.rounds());
    let mut specialized_ns = Vec::with_capacity(config.mode.rounds());
    for round in 0..config.mode.rounds() {
        let (generic, specialized, observed_checksum) = run_once(
            &words,
            &mut generic_out,
            &mut specialized_out,
            round % 2 == 0,
        )?;
        if observed_checksum != expected_checksum {
            return Err("packed semantic checksum changed across rounds".to_owned());
        }
        generic_ns.push(generic);
        specialized_ns.push(specialized);
    }

    let generic = summarize(generic_ns);
    let specialized = summarize(specialized_ns);
    println!(
        "{{\"schema\":1,\"kind\":\"r2c-packed-four-bit-specialization-probe\",\"mode\":\"{}\",\"diagnostic_only\":true,\"performance_admitted\":false,\"alternating_order\":true,\"rounds\":{},\"cells\":{},\"bits_per_entry\":{},\"palette_entries\":{},\"error_cell\":{},\"error_index\":{},\"checksum\":{},\"runtime_generic_ns\":{},\"four_bit_specialized_ns\":{},\"p50_ratio_milli\":{},\"hardware\":{}}}",
        config.mode.as_str(),
        config.mode.rounds(),
        CELLS,
        FOUR_BITS,
        PALETTE.len(),
        ERROR_CELL,
        ERROR_INDEX,
        expected_checksum,
        summary_json(generic),
        summary_json(specialized),
        ratio_milli(specialized.p50, generic.p50),
        hardware.to_json(),
    );
    Ok(())
}

fn run_once(
    words: &[u64; WORDS],
    generic_out: &mut Vec<u16>,
    specialized_out: &mut Vec<u16>,
    generic_first: bool,
) -> Result<(u128, u128, u64), String> {
    let (generic_ns, specialized_ns) = if generic_first {
        let generic = measure_runtime_generic(words, generic_out)?;
        let specialized = measure_four_bit_specialized(words, specialized_out)?;
        (generic, specialized)
    } else {
        let specialized = measure_four_bit_specialized(words, specialized_out)?;
        let generic = measure_runtime_generic(words, generic_out)?;
        (generic, specialized)
    };

    if generic_out != specialized_out {
        return Err("four-bit specialization diverged during measurement".to_owned());
    }
    let observed = checksum(black_box(specialized_out));
    black_box(generic_out);
    Ok((generic_ns, specialized_ns, observed))
}

fn measure_runtime_generic(words: &[u64; WORDS], out: &mut Vec<u16>) -> Result<u128, String> {
    out.clear();
    let bits_per_entry = black_box(FOUR_BITS);
    let start = Instant::now();
    decode_runtime_generic(black_box(words), &PALETTE, bits_per_entry, out)
        .map_err(|error| format!("generic measured decode failed: {error:?}"))?;
    Ok(start.elapsed().as_nanos())
}

fn measure_four_bit_specialized(words: &[u64; WORDS], out: &mut Vec<u16>) -> Result<u128, String> {
    out.clear();
    let start = Instant::now();
    decode_four_bit_specialized(black_box(words), &PALETTE, out)
        .map_err(|error| format!("specialized measured decode failed: {error:?}"))?;
    Ok(start.elapsed().as_nanos())
}

fn decode_runtime_generic(
    words: &[u64; WORDS],
    palette: &[u16],
    bits_per_entry: usize,
    out: &mut Vec<u16>,
) -> Result<(), ProbeError> {
    let values_per_word = u64::BITS as usize / bits_per_entry;
    let mask = (1_u64 << bits_per_entry) - 1;
    for cell in 0..CELLS {
        let word = words[cell / values_per_word];
        let shift = (cell % values_per_word) * bits_per_entry;
        let raw_palette_index = (word >> shift) & mask;
        let palette_index =
            usize::try_from(raw_palette_index).map_err(|_| ProbeError::ArithmeticOverflow)?;
        let state =
            palette
                .get(palette_index)
                .copied()
                .ok_or(ProbeError::PaletteIndexOutOfRange {
                    cell,
                    palette_index,
                    palette_entries: palette.len(),
                })?;
        out.push(state);
    }
    Ok(())
}

fn decode_four_bit_specialized(
    words: &[u64; WORDS],
    palette: &[u16],
    out: &mut Vec<u16>,
) -> Result<(), ProbeError> {
    for cell in 0..CELLS {
        let word = words[cell >> 4];
        let shift = (cell & 0x0f) << 2;
        let raw_palette_index = (word >> shift) & FOUR_MASK;
        let palette_index =
            usize::try_from(raw_palette_index).map_err(|_| ProbeError::ArithmeticOverflow)?;
        let state =
            palette
                .get(palette_index)
                .copied()
                .ok_or(ProbeError::PaletteIndexOutOfRange {
                    cell,
                    palette_index,
                    palette_entries: palette.len(),
                })?;
        out.push(state);
    }
    Ok(())
}

fn validate_error_equivalence(mut words: [u64; WORDS]) -> Result<(), String> {
    let word_index = ERROR_CELL >> 4;
    let shift = (ERROR_CELL & 0x0f) << 2;
    words[word_index] &= !(FOUR_MASK << shift);
    words[word_index] |= (ERROR_INDEX as u64) << shift;

    let expected = ProbeError::PaletteIndexOutOfRange {
        cell: ERROR_CELL,
        palette_index: ERROR_INDEX,
        palette_entries: PALETTE.len(),
    };
    let mut generic_out = Vec::with_capacity(CELLS);
    let mut specialized_out = Vec::with_capacity(CELLS);
    let generic = decode_runtime_generic(&words, &PALETTE, black_box(FOUR_BITS), &mut generic_out);
    let specialized = decode_four_bit_specialized(&words, &PALETTE, &mut specialized_out);
    if generic != Err(expected) || specialized != Err(expected) {
        return Err(format!(
            "error semantics diverged: generic={generic:?} specialized={specialized:?} expected={expected:?}"
        ));
    }
    if generic_out.len() != ERROR_CELL || specialized_out.len() != ERROR_CELL {
        return Err(format!(
            "decoders did not stop at exact failing cell: generic={} specialized={} expected={ERROR_CELL}",
            generic_out.len(),
            specialized_out.len()
        ));
    }
    Ok(())
}

fn fixture_words() -> [u64; WORDS] {
    let mut words = [0_u64; WORDS];
    for cell in 0..CELLS {
        let index = u64::from(((cell ^ (cell >> 4)) & 1) != 0);
        let word = cell >> 4;
        let shift = (cell & 0x0f) << 2;
        words[word] |= index << shift;
    }
    words
}

fn checksum(states: &[u16]) -> u64 {
    states.iter().enumerate().fold(0_u64, |acc, (cell, state)| {
        acc.wrapping_mul(0x9E37_79B1_85EB_CA87)
            .wrapping_add((cell as u64) << 1)
            .wrapping_add(u64::from(*state))
    })
}

fn parse_args() -> Result<Config, String> {
    let mut mode = None;
    let mut require_single_cpu = false;
    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--require-single-cpu" => require_single_cpu = true,
            "--help" | "-h" => {
                return Err(
                    "usage: r2c_packed_four_bit_probe (--smoke|--full) [--require-single-cpu]"
                        .to_owned(),
                );
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    Ok(Config {
        mode: mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?,
        require_single_cpu,
    })
}

fn set_mode(slot: &mut Option<Mode>, mode: Mode) -> Result<(), String> {
    if slot.replace(mode).is_some() {
        return Err("specify exactly one benchmark mode".to_owned());
    }
    Ok(())
}

fn summarize(mut values: Vec<u128>) -> Summary {
    values.sort_unstable();
    Summary {
        p50: percentile(&values, 50),
        p95: percentile(&values, 95),
        p99: percentile(&values, 99),
        max: values.last().copied().unwrap_or_default(),
    }
}

fn percentile(values: &[u128], percentile: usize) -> u128 {
    if values.is_empty() {
        return 0;
    }
    let index = (values.len() - 1) * percentile / 100;
    values[index]
}

fn ratio_milli(numerator: u128, denominator: u128) -> u128 {
    if denominator == 0 {
        return 0;
    }
    numerator.saturating_mul(1000) / denominator
}

fn summary_json(summary: Summary) -> String {
    format!(
        "{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{}}}",
        summary.p50, summary.p95, summary.p99, summary.max
    )
}
