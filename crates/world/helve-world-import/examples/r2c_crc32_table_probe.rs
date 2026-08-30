use std::{
    env, fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::Instant,
};

use helve_world_import::{
    ChunkCompression, ChunkPayloadDecoder, DeflateChunkPayloadDecoder, RegionLimits, RegionView,
};

const SECTOR_BYTES: usize = 4096;
const MAX_REGION_BYTES: usize = 16 * SECTOR_BYTES;
const MAX_INLINE_COMPRESSED_BYTES: usize = 4 * SECTOR_BYTES;
const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024;
const PACKED_LOCAL_X: u8 = 1;
const PACKED_LOCAL_Z: u8 = 0;
const GZIP_TRAILER_BYTES: usize = 8;
const CRC32_POLYNOMIAL: u32 = 0xedb8_8320;
const BATCH: usize = 64;

const CRC32_NIBBLE_TABLE: [u32; 16] = [
    0x0000_0000,
    0x1db7_1064,
    0x3b6e_20c8,
    0x26d9_30ac,
    0x76dc_4190,
    0x6b6b_51f4,
    0x4db2_6158,
    0x5005_713c,
    0xedb8_8320,
    0xf00f_9344,
    0xd6d6_a3e8,
    0xcb61_b38c,
    0x9b64_c2b0,
    0x86d3_d2d4,
    0xa00a_e278,
    0xbdbd_f21c,
];

const CRC32_BYTE_TABLE: [u32; 256] = make_crc32_byte_table();

const fn make_crc32_byte_table() -> [u32; 256] {
    let mut table = [0_u32; 256];
    let mut index = 0_usize;
    while index < table.len() {
        let mut crc = index as u32;
        let mut bit = 0_u8;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                CRC32_POLYNOMIAL ^ (crc >> 1)
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

#[derive(Clone, Copy)]
enum Mode {
    Smoke,
    Full,
}

impl Mode {
    const fn rounds(self) -> usize {
        match self {
            Self::Smoke => 1024,
            Self::Full => 16_384,
        }
    }

    const fn warmups(self) -> usize {
        match self {
            Self::Smoke => 128,
            Self::Full => 1024,
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
    packed_region: PathBuf,
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
        eprintln!("r2c CRC32 table probe failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let (output, expected_crc) = load_packed_output(&config.packed_region)?;
    let nibble_crc = crc32_nibble(&output);
    let byte_crc = crc32_byte(&output);
    if nibble_crc != expected_crc || byte_crc != expected_crc {
        return Err(format!(
            "CRC candidate disagreement: expected={expected_crc:#010x} nibble={nibble_crc:#010x} byte={byte_crc:#010x}"
        ));
    }

    for round in 0..config.mode.warmups() {
        black_box(measure_pair(&output, round % 2 == 0));
    }

    let mut nibble_ns = Vec::with_capacity(config.mode.rounds());
    let mut byte_ns = Vec::with_capacity(config.mode.rounds());
    for round in 0..config.mode.rounds() {
        let (nibble, byte) = measure_pair(&output, round % 2 == 0);
        nibble_ns.push(nibble);
        byte_ns.push(byte);
    }

    let nibble = summarize(nibble_ns);
    let byte = summarize(byte_ns);
    println!(
        "{{\"schema\":1,\"kind\":\"r2c-crc32-table-probe\",\"mode\":\"{}\",\"fixture\":\"differential-packed4-gzip\",\"diagnostic_only\":true,\"performance_admitted\":false,\"alternating_order\":true,\"rounds\":{},\"batch\":{},\"bytes\":{},\"crc32\":{},\"nibble_table_bytes\":{},\"byte_table_bytes\":{},\"nibble_ns\":{},\"byte_ns\":{},\"p50_ratio_milli\":{}}}",
        config.mode.as_str(),
        config.mode.rounds(),
        BATCH,
        output.len(),
        expected_crc,
        std::mem::size_of_val(&CRC32_NIBBLE_TABLE),
        std::mem::size_of_val(&CRC32_BYTE_TABLE),
        summary_json(nibble),
        summary_json(byte),
        ratio_milli(byte.p50, nibble.p50),
    );
    Ok(())
}

fn load_packed_output(path: &Path) -> Result<(Vec<u8>, u32), String> {
    let region_bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if region_bytes.len() > MAX_REGION_BYTES {
        return Err(format!(
            "packed region exceeds {MAX_REGION_BYTES} bytes: {}",
            region_bytes.len()
        ));
    }
    let region = RegionView::new(
        &region_bytes,
        0,
        0,
        RegionLimits::new(MAX_REGION_BYTES, MAX_INLINE_COMPRESSED_BYTES),
    )
    .map_err(|error| format!("packed region rejected: {error:?}"))?;
    let chunk = region
        .chunk(PACKED_LOCAL_X, PACKED_LOCAL_Z)
        .map_err(|error| format!("packed chunk framing failed: {error:?}"))?
        .ok_or_else(|| "packed chunk slot is empty".to_owned())?;
    if chunk.compression != ChunkCompression::Gzip || chunk.external {
        return Err(format!(
            "packed fixture must be inline gzip, observed compression={:?} external={}",
            chunk.compression, chunk.external
        ));
    }
    let payload = chunk
        .inline_payload
        .ok_or_else(|| "inline packed chunk omitted payload".to_owned())?;
    if payload.len() < GZIP_TRAILER_BYTES {
        return Err("packed gzip payload is shorter than trailer".to_owned());
    }
    let trailer = &payload[payload.len() - GZIP_TRAILER_BYTES..];
    let expected_crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);

    let mut decoder = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("decoder init failed: {error:?}"))?;
    let output = decoder
        .decode(ChunkCompression::Gzip, payload, MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("production gzip decode failed: {error:?}"))?;
    Ok((output.to_vec(), expected_crc))
}

fn measure_pair(bytes: &[u8], nibble_first: bool) -> (u128, u128) {
    if nibble_first {
        (measure_nibble(bytes), measure_byte(bytes))
    } else {
        let byte = measure_byte(bytes);
        let nibble = measure_nibble(bytes);
        (nibble, byte)
    }
}

fn measure_nibble(bytes: &[u8]) -> u128 {
    measure_batch(|| black_box(crc32_nibble(black_box(bytes))))
}

fn measure_byte(bytes: &[u8]) -> u128 {
    measure_batch(|| black_box(crc32_byte(black_box(bytes))))
}

fn measure_batch<F, T>(mut operation: F) -> u128
where
    F: FnMut() -> T,
{
    let start = Instant::now();
    for _ in 0..BATCH {
        black_box(operation());
    }
    start.elapsed().as_nanos() / BATCH as u128
}

fn crc32_nibble(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        let low = usize::from(crc.to_le_bytes()[0] & 0x0f);
        crc = CRC32_NIBBLE_TABLE[low] ^ (crc >> 4);
        let high = usize::from(crc.to_le_bytes()[0] & 0x0f);
        crc = CRC32_NIBBLE_TABLE[high] ^ (crc >> 4);
    }
    !crc
}

fn crc32_byte(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        let index = usize::from((crc as u8) ^ byte);
        crc = CRC32_BYTE_TABLE[index] ^ (crc >> 8);
    }
    !crc
}

fn parse_args() -> Result<Config, String> {
    let mut mode = None;
    let mut packed_region = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--smoke" => set_mode(&mut mode, Mode::Smoke)?,
            "--full" => set_mode(&mut mode, Mode::Full)?,
            "--packed4-region" => {
                if packed_region.is_some() {
                    return Err("--packed4-region may be specified only once".to_owned());
                }
                let path = args
                    .next()
                    .ok_or_else(|| "--packed4-region requires a path".to_owned())?;
                packed_region = Some(PathBuf::from(path));
            }
            "--help" | "-h" => {
                return Err(
                    "usage: r2c_crc32_table_probe (--smoke|--full) --packed4-region PATH".to_owned(),
                );
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    Ok(Config {
        mode: mode.ok_or_else(|| "exactly one of --smoke or --full is required".to_owned())?,
        packed_region: packed_region
            .ok_or_else(|| "--packed4-region PATH is required".to_owned())?,
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
