use std::{
    env, fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::Instant,
};

use helve_world_import::{
    ChunkCompression, ChunkPayloadDecoder, DeflateChunkPayloadDecoder, RegionLimits, RegionView,
};
use miniz_oxide::inflate::{
    TINFLStatus,
    core::{
        DecompressorOxide, decompress, inflate_flags::TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    },
};

const SECTOR_BYTES: usize = 4096;
const MAX_REGION_BYTES: usize = 16 * SECTOR_BYTES;
const MAX_INLINE_COMPRESSED_BYTES: usize = 4 * SECTOR_BYTES;
const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024;
const PACKED_LOCAL_X: u8 = 1;
const PACKED_LOCAL_Z: u8 = 0;
const GZIP_FIXED_HEADER_BYTES: usize = 10;
const GZIP_TRAILER_BYTES: usize = 8;
const GZIP_FLAG_HEADER_CRC: u8 = 0x02;
const GZIP_FLAG_EXTRA: u8 = 0x04;
const GZIP_FLAG_NAME: u8 = 0x08;
const GZIP_FLAG_COMMENT: u8 = 0x10;
const GZIP_RESERVED_FLAGS: u8 = 0xe0;
const BATCH: usize = 32;

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

#[derive(Clone, Copy)]
enum Mode {
    Smoke,
    Full,
}

impl Mode {
    const fn rounds(self) -> usize {
        match self {
            Self::Smoke => 128,
            Self::Full => 2048,
        }
    }

    const fn warmups(self) -> usize {
        match self {
            Self::Smoke => 16,
            Self::Full => 128,
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
struct GzipMember {
    body_start: usize,
    trailer_start: usize,
    expected_crc32: u32,
    expected_size: u32,
}

#[derive(Clone, Copy)]
struct Summary {
    p50: u128,
    p95: u128,
    p99: u128,
    max: u128,
}

#[derive(Default)]
struct Samples {
    production_total: Vec<u128>,
    diagnostic_total: Vec<u128>,
    framing: Vec<u128>,
    raw_inflate: Vec<u128>,
    crc32: Vec<u128>,
    size_check: Vec<u128>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("r2c gzip decode component diagnostic failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let config = parse_args()?;
    let payload = load_packed_gzip(&config.packed_region)?;
    let member = parse_gzip_member(&payload)?;
    let body = payload
        .get(member.body_start..member.trailer_start)
        .ok_or_else(|| "gzip body slice is invalid".to_owned())?;

    let mut production = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("production decoder init failed: {error:?}"))?;
    let mut diagnostic_decompressor = DecompressorOxide::new();
    let mut diagnostic_output = vec![0_u8; MAX_DECOMPRESSED_BYTES];
    let (production_len, production_checksum) = validate_equivalence(
        &payload,
        member,
        body,
        &mut production,
        &mut diagnostic_decompressor,
        &mut diagnostic_output,
    )?;

    let samples = collect_samples(
        config.mode,
        &payload,
        member,
        &mut production,
        &mut diagnostic_decompressor,
        &mut diagnostic_output,
    )?;
    report(
        config.mode,
        payload.len(),
        body.len(),
        production_len,
        production_checksum,
        samples,
    );
    Ok(())
}

fn load_packed_gzip(path: &Path) -> Result<Vec<u8>, String> {
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
    chunk
        .inline_payload
        .map(<[u8]>::to_vec)
        .ok_or_else(|| "inline packed chunk omitted payload".to_owned())
}

fn validate_equivalence(
    payload: &[u8],
    member: GzipMember,
    body: &[u8],
    production: &mut DeflateChunkPayloadDecoder,
    diagnostic_decompressor: &mut DecompressorOxide,
    diagnostic_output: &mut [u8],
) -> Result<(usize, u64), String> {
    let production_output = production
        .decode(ChunkCompression::Gzip, payload, MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("production gzip decode failed: {error:?}"))?;
    let production_len = production_output.len();
    let production_checksum = byte_checksum(production_output);

    let diagnostic_len = inflate_raw(diagnostic_decompressor, body, diagnostic_output)?;
    validate_trailer(member, &diagnostic_output[..diagnostic_len])?;
    if diagnostic_len != production_len
        || byte_checksum(&diagnostic_output[..diagnostic_len]) != production_checksum
        || &diagnostic_output[..diagnostic_len] != production_output
    {
        return Err("diagnostic gzip path disagrees with production output".to_owned());
    }
    Ok((production_len, production_checksum))
}

fn collect_samples(
    mode: Mode,
    payload: &[u8],
    member: GzipMember,
    production: &mut DeflateChunkPayloadDecoder,
    diagnostic_decompressor: &mut DecompressorOxide,
    diagnostic_output: &mut [u8],
) -> Result<Samples, String> {
    for _ in 0..mode.warmups() {
        black_box(measure_round(
            payload,
            member,
            production,
            diagnostic_decompressor,
            diagnostic_output,
        )?);
    }

    let mut samples = Samples {
        production_total: Vec::with_capacity(mode.rounds()),
        diagnostic_total: Vec::with_capacity(mode.rounds()),
        framing: Vec::with_capacity(mode.rounds()),
        raw_inflate: Vec::with_capacity(mode.rounds()),
        crc32: Vec::with_capacity(mode.rounds()),
        size_check: Vec::with_capacity(mode.rounds()),
    };
    for _ in 0..mode.rounds() {
        let round = measure_round(
            payload,
            member,
            production,
            diagnostic_decompressor,
            diagnostic_output,
        )?;
        samples.production_total.push(round[0]);
        samples.diagnostic_total.push(round[1]);
        samples.framing.push(round[2]);
        samples.raw_inflate.push(round[3]);
        samples.crc32.push(round[4]);
        samples.size_check.push(round[5]);
    }
    Ok(samples)
}

fn report(
    mode: Mode,
    payload_len: usize,
    body_len: usize,
    output_len: usize,
    output_checksum: u64,
    samples: Samples,
) {
    let production_total = summarize(samples.production_total);
    let diagnostic_total = summarize(samples.diagnostic_total);
    let framing = summarize(samples.framing);
    let raw_inflate = summarize(samples.raw_inflate);
    let crc32 = summarize(samples.crc32);
    let size_check = summarize(samples.size_check);
    let accounted_p50 = framing
        .p50
        .saturating_add(raw_inflate.p50)
        .saturating_add(crc32.p50)
        .saturating_add(size_check.p50);

    println!(
        "{{\"schema\":1,\"kind\":\"r2c-gzip-decode-components\",\"mode\":\"{}\",\"fixture\":\"differential-packed4-gzip\",\"diagnostic_only\":true,\"performance_admitted\":false,\"rounds\":{},\"batch\":{},\"compressed_bytes\":{},\"deflate_bytes\":{},\"decompressed_bytes\":{},\"output_checksum\":{},\"production_total_ns\":{},\"diagnostic_total_ns\":{},\"framing_ns\":{},\"raw_inflate_ns\":{},\"crc32_ns\":{},\"size_check_ns\":{},\"accounted_p50_ns\":{},\"diagnostic_to_production_p50_milli\":{}}}",
        mode.as_str(),
        mode.rounds(),
        BATCH,
        payload_len,
        body_len,
        output_len,
        output_checksum,
        summary_json(production_total),
        summary_json(diagnostic_total),
        summary_json(framing),
        summary_json(raw_inflate),
        summary_json(crc32),
        summary_json(size_check),
        accounted_p50,
        ratio_milli(diagnostic_total.p50, production_total.p50),
    );
}

fn measure_round(
    payload: &[u8],
    member: GzipMember,
    production: &mut DeflateChunkPayloadDecoder,
    diagnostic_decompressor: &mut DecompressorOxide,
    diagnostic_output: &mut [u8],
) -> Result<[u128; 6], String> {
    let production_total = measure_batch(|| {
        let output = production
            .decode(
                ChunkCompression::Gzip,
                black_box(payload),
                MAX_DECOMPRESSED_BYTES,
            )
            .map_err(|error| format!("production gzip decode failed: {error:?}"))?;
        black_box(output.len());
        Ok(())
    })?;

    let diagnostic_total = measure_batch(|| {
        let parsed = parse_gzip_member(black_box(payload))?;
        let body = payload
            .get(parsed.body_start..parsed.trailer_start)
            .ok_or_else(|| "diagnostic gzip body slice invalid".to_owned())?;
        let written = inflate_raw(diagnostic_decompressor, body, diagnostic_output)?;
        validate_trailer(parsed, &diagnostic_output[..written])?;
        black_box(written);
        Ok(())
    })?;

    let framing = measure_batch(|| {
        black_box(parse_gzip_member(black_box(payload))?);
        Ok(())
    })?;

    let body = payload
        .get(member.body_start..member.trailer_start)
        .ok_or_else(|| "gzip body slice invalid".to_owned())?;
    let raw_inflate = measure_batch(|| {
        let written = inflate_raw(diagnostic_decompressor, black_box(body), diagnostic_output)?;
        black_box(written);
        Ok(())
    })?;

    let written = inflate_raw(diagnostic_decompressor, body, diagnostic_output)?;
    let output = &diagnostic_output[..written];
    let crc32_time = measure_batch(|| {
        black_box(crc32(black_box(output)));
        Ok(())
    })?;
    let size_check = measure_batch(|| {
        let actual_size = u32::try_from(black_box(output.len()))
            .map_err(|_| "diagnostic output length exceeds u32".to_owned())?;
        if actual_size != black_box(member.expected_size) {
            return Err("diagnostic gzip ISIZE mismatch".to_owned());
        }
        black_box(actual_size);
        Ok(())
    })?;

    Ok([
        production_total,
        diagnostic_total,
        framing,
        raw_inflate,
        crc32_time,
        size_check,
    ])
}

fn measure_batch<F>(mut operation: F) -> Result<u128, String>
where
    F: FnMut() -> Result<(), String>,
{
    let start = Instant::now();
    for _ in 0..BATCH {
        operation()?;
    }
    Ok(start.elapsed().as_nanos() / BATCH as u128)
}

fn inflate_raw(
    decompressor: &mut DecompressorOxide,
    body: &[u8],
    output: &mut [u8],
) -> Result<usize, String> {
    decompressor.init();
    let (status, consumed, written) = decompress(
        decompressor,
        body,
        output,
        0,
        TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
    );
    if status != TINFLStatus::Done {
        return Err(format!("diagnostic raw inflate failed: {status:?}"));
    }
    if consumed != body.len() {
        return Err(format!(
            "diagnostic raw inflate left {} trailing bytes",
            body.len() - consumed
        ));
    }
    Ok(written)
}

fn validate_trailer(member: GzipMember, output: &[u8]) -> Result<(), String> {
    let actual_crc = crc32(output);
    if actual_crc != member.expected_crc32 {
        return Err(format!(
            "diagnostic gzip CRC mismatch: expected={:#010x} actual={:#010x}",
            member.expected_crc32, actual_crc
        ));
    }
    let actual_size = u32::try_from(output.len())
        .map_err(|_| "diagnostic output length exceeds u32".to_owned())?;
    if actual_size != member.expected_size {
        return Err(format!(
            "diagnostic gzip ISIZE mismatch: expected={} actual={actual_size}",
            member.expected_size
        ));
    }
    Ok(())
}

fn parse_gzip_member(payload: &[u8]) -> Result<GzipMember, String> {
    if payload.len() < GZIP_FIXED_HEADER_BYTES + GZIP_TRAILER_BYTES {
        return Err(format!("gzip payload too short: {}", payload.len()));
    }
    if payload.get(..2) != Some(&[0x1f, 0x8b][..]) {
        return Err("invalid gzip magic".to_owned());
    }
    if payload[2] != 8 {
        return Err(format!("unsupported gzip method: {}", payload[2]));
    }
    let flags = payload[3];
    if flags & GZIP_RESERVED_FLAGS != 0 {
        return Err(format!("reserved gzip flags set: {flags:#04x}"));
    }

    let trailer_start = payload.len() - GZIP_TRAILER_BYTES;
    let mut cursor = GZIP_FIXED_HEADER_BYTES;
    if flags & GZIP_FLAG_EXTRA != 0 {
        let length_end = cursor
            .checked_add(2)
            .filter(|&end| end <= trailer_start)
            .ok_or_else(|| "malformed gzip extra header".to_owned())?;
        let length = payload
            .get(cursor..length_end)
            .ok_or_else(|| "malformed gzip extra length".to_owned())?;
        let xlen = usize::from(u16::from_le_bytes([length[0], length[1]]));
        cursor = length_end
            .checked_add(xlen)
            .filter(|&end| end <= trailer_start)
            .ok_or_else(|| "malformed gzip extra field".to_owned())?;
    }
    if flags & GZIP_FLAG_NAME != 0 {
        cursor = skip_zero_terminated(payload, cursor, trailer_start)?;
    }
    if flags & GZIP_FLAG_COMMENT != 0 {
        cursor = skip_zero_terminated(payload, cursor, trailer_start)?;
    }
    if flags & GZIP_FLAG_HEADER_CRC != 0 {
        let crc_end = cursor
            .checked_add(2)
            .filter(|&end| end <= trailer_start)
            .ok_or_else(|| "malformed gzip header CRC".to_owned())?;
        let expected = u16::from_le_bytes([payload[cursor], payload[cursor + 1]]);
        let actual_bytes = crc32(&payload[..cursor]).to_le_bytes();
        let actual = u16::from_le_bytes([actual_bytes[0], actual_bytes[1]]);
        if expected != actual {
            return Err(format!(
                "gzip header CRC mismatch: expected={expected:#06x} actual={actual:#06x}"
            ));
        }
        cursor = crc_end;
    }

    let trailer = payload
        .get(trailer_start..)
        .ok_or_else(|| "gzip trailer missing".to_owned())?;
    Ok(GzipMember {
        body_start: cursor,
        trailer_start,
        expected_crc32: u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]),
        expected_size: u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]),
    })
}

fn skip_zero_terminated(payload: &[u8], start: usize, end: usize) -> Result<usize, String> {
    let bytes = payload
        .get(start..end)
        .ok_or_else(|| "malformed zero-terminated gzip field".to_owned())?;
    let terminator = bytes
        .iter()
        .position(|&byte| byte == 0)
        .ok_or_else(|| "unterminated gzip field".to_owned())?;
    start
        .checked_add(terminator)
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| "gzip header arithmetic overflow".to_owned())
}

fn crc32(bytes: &[u8]) -> u32 {
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

fn byte_checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        hash.wrapping_mul(0x0000_0100_0000_01b3) ^ u64::from(*byte)
    })
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
                    "usage: r2c_gzip_decode_components (--smoke|--full) --packed4-region PATH"
                        .to_owned(),
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
