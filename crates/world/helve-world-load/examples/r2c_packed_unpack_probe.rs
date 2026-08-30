use std::{env, hint::black_box, time::Instant};

use helve_benchmark_support::collect_hardware_metadata;

const CELLS: usize = 4096;
const BITS: usize = 4;
const VALUES_PER_WORD: usize = 64 / BITS;
const WORDS: usize = CELLS / VALUES_PER_WORD;
const MASK: u64 = (1_u64 << BITS) - 1;
const PALETTE: [u16; 2] = [0, 1];

#[derive(Clone, Copy)]
enum Mode {
    Smoke,
    Full,
}

impl Mode {
    const fn rounds(self) -> usize {
        match self {
            Self::Smoke => 64,
            Self::Full => 2048,
        }
    }

    const fn warmups(self) -> usize {
        match self {
            Self::Smoke => 16,
            Self::Full => 256,
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

#[derive(Clone, Copy)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("r2c packed unpack probe failed: {error}");
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
    let mut cell_major_out = Vec::with_capacity(CELLS);
    let mut word_major_out = Vec::with_capacity(CELLS);

    decode_cell_major(&words, &mut cell_major_out)?;
    decode_word_major(&words, &mut word_major_out)?;
    if cell_major_out != word_major_out {
        return Err("word-major decode changed packed semantics".to_owned());
    }
    let checksum = checksum(&cell_major_out);

    for _ in 0..config.mode.warmups() {
        black_box(run_once(&words, &mut cell_major_out, &mut word_major_out)?);
    }

    let mut cell_major_ns = Vec::with_capacity(config.mode.rounds());
    let mut word_major_ns = Vec::with_capacity(config.mode.rounds());
    for _ in 0..config.mode.rounds() {
        let (cell_ns, word_ns, observed) =
            run_once(&words, &mut cell_major_out, &mut word_major_out)?;
        if observed != checksum {
            return Err("packed checksum changed across rounds".to_owned());
        }
        cell_major_ns.push(cell_ns);
        word_major_ns.push(word_ns);
    }

    let cell = summarize(cell_major_ns);
    let word = summarize(word_major_ns);
    println!(
        "{{\"schema\":1,\"kind\":\"r2c-packed-unpack-loop-probe\",\"mode\":\"{}\",\"diagnostic_only\":true,\"performance_admitted\":false,\"rounds\":{},\"cells\":{},\"bits_per_entry\":{},\"checksum\":{},\"cell_major_ns\":{},\"word_major_ns\":{},\"p50_ratio_milli\":{},\"hardware\":{}}}",
        config.mode.as_str(),
        config.mode.rounds(),
        CELLS,
        BITS,
        checksum,
        summary_json(cell),
        summary_json(word),
        ratio_milli(word.p50, cell.p50),
        hardware.to_json(),
    );
    Ok(())
}

fn run_once(
    words: &[u64; WORDS],
    cell_major_out: &mut Vec<u16>,
    word_major_out: &mut Vec<u16>,
) -> Result<(u128, u128, u64), String> {
    cell_major_out.clear();
    let start = Instant::now();
    decode_cell_major(black_box(words), cell_major_out)?;
    let cell_ns = start.elapsed().as_nanos();

    word_major_out.clear();
    let start = Instant::now();
    decode_word_major(black_box(words), word_major_out)?;
    let word_ns = start.elapsed().as_nanos();

    if cell_major_out != word_major_out {
        return Err("word-major decode diverged during measurement".to_owned());
    }
    let observed = checksum(black_box(word_major_out));
    black_box(cell_major_out);
    Ok((cell_ns, word_ns, observed))
}

fn decode_cell_major(words: &[u64; WORDS], out: &mut Vec<u16>) -> Result<(), String> {
    for cell in 0..CELLS {
        let word = words[cell / VALUES_PER_WORD];
        let shift = (cell % VALUES_PER_WORD) * BITS;
        let raw = (word >> shift) & MASK;
        let index = usize::try_from(raw).map_err(|_| "palette index conversion failed")?;
        let state = PALETTE
            .get(index)
            .copied()
            .ok_or_else(|| format!("palette index {index} out of range at cell {cell}"))?;
        out.push(state);
    }
    Ok(())
}

fn decode_word_major(words: &[u64; WORDS], out: &mut Vec<u16>) -> Result<(), String> {
    let mut cell = 0_usize;
    for &packed in words {
        let mut word = packed;
        for _ in 0..VALUES_PER_WORD {
            let raw = word & MASK;
            let index = usize::try_from(raw).map_err(|_| "palette index conversion failed")?;
            let state = PALETTE
                .get(index)
                .copied()
                .ok_or_else(|| format!("palette index {index} out of range at cell {cell}"))?;
            out.push(state);
            word >>= BITS;
            cell += 1;
        }
    }
    if cell != CELLS {
        return Err(format!("word-major decode produced {cell} cells"));
    }
    Ok(())
}

fn fixture_words() -> [u64; WORDS] {
    let mut words = [0_u64; WORDS];
    for cell in 0..CELLS {
        let index = u64::from(((cell ^ (cell >> 4)) & 1) != 0);
        let word = cell / VALUES_PER_WORD;
        let shift = (cell % VALUES_PER_WORD) * BITS;
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
                    "usage: r2c_packed_unpack_probe (--smoke|--full) [--require-single-cpu]"
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
