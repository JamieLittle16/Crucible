//! Qualification-only normalized section fact emitter for one Anvil region.
//!
//! This binary intentionally uses the production Rust importer and emits a tiny semantic surface for
//! comparison with the independent Python vanilla-save oracle. It does not share NBT parsing,
//! decompression, state resolution, or packed-section decoding with that oracle.

use std::{
    env,
    fs::File,
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use helve_generated::BlockStateId;
use helve_world_import::{
    BlockSectionDecodeScratch, ChunkPayloadLimits, DeflateChunkPayloadDecoder,
    ExternalChunkPayload, ImportedBlockSectionBuilder, NbtLimits, RegionLimits, RegionView,
    StoredBlockImporter, Target262BlockStateResolver,
};

const MAX_REGION_BYTES: usize = 256 * 1024 * 1024;
const MAX_INLINE_COMPRESSED_BYTES: usize = 8 * 1024 * 1024;
const MAX_EXTERNAL_COMPRESSED_BYTES: usize = 16 * 1024 * 1024;
const MAX_DECOMPRESSED_BYTES: usize = 16 * 1024 * 1024;
const MAX_NBT_STRING_BYTES: usize = 1024 * 1024;
const MAX_NBT_LIST_ELEMENTS: usize = 1024 * 1024;
const MAX_NBT_ARRAY_ELEMENTS: usize = 16 * 1024 * 1024;
const MAX_NBT_DEPTH: usize = 64;

#[derive(Debug)]
struct DenseSectionBuilder;

impl ImportedBlockSectionBuilder<BlockStateId> for DenseSectionBuilder {
    type Section = Vec<BlockStateId>;

    fn build_uniform(&mut self, state: BlockStateId) -> Self::Section {
        vec![state; 4096]
    }

    fn build_states(&mut self, states: &[BlockStateId]) -> Self::Section {
        states.to_vec()
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("r2c import section facts: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let dimension = args
        .next()
        .ok_or_else(|| usage("missing dimension"))?
        .into_string()
        .map_err(|_| usage("dimension is not UTF-8"))?;
    let region_path = PathBuf::from(args.next().ok_or_else(|| usage("missing region path"))?);
    let output_path = PathBuf::from(args.next().ok_or_else(|| usage("missing output path"))?);
    if args.next().is_some() {
        return Err(usage("too many arguments"));
    }
    validate_dimension(&dimension)?;
    let (region_x, region_z) = parse_region_coordinates(&region_path)?;
    let region_bytes = read_bounded(&region_path, MAX_REGION_BYTES)?;
    let region = RegionView::new(
        &region_bytes,
        region_x,
        region_z,
        RegionLimits::new(MAX_REGION_BYTES, MAX_INLINE_COMPRESSED_BYTES),
    )
    .map_err(|error| format!("invalid region {}: {error:?}", region_path.display()))?;

    let nbt_limits = NbtLimits::new(
        MAX_NBT_STRING_BYTES,
        MAX_NBT_LIST_ELEMENTS,
        MAX_NBT_ARRAY_ELEMENTS,
        MAX_NBT_DEPTH,
    )
    .map_err(|error| format!("invalid qualification NBT limits: {error:?}"))?;
    let payload_limits =
        ChunkPayloadLimits::new(MAX_EXTERNAL_COMPRESSED_BYTES, MAX_DECOMPRESSED_BYTES);
    let mut decoder = DeflateChunkPayloadDecoder::try_with_output_limit(MAX_DECOMPRESSED_BYTES)
        .map_err(|error| format!("could not initialize decoder scratch: {error:?}"))?;
    let resolver = Target262BlockStateResolver;
    let mut builder = DenseSectionBuilder;
    let mut scratch = BlockSectionDecodeScratch::new();
    let mut importer = StoredBlockImporter::new(
        payload_limits,
        nbt_limits,
        &mut decoder,
        &resolver,
        &mut builder,
        &mut scratch,
    );

    let output = File::create(&output_path)
        .map_err(|error| format!("could not create {}: {error}", output_path.display()))?;
    let mut output = BufWriter::new(output);

    // x-major, then z-major matches the oracle's canonical semantic ordering inside one region.
    for local_x in 0_u8..32 {
        for local_z in 0_u8..32 {
            let Some(chunk) = region
                .chunk(local_x, local_z)
                .map_err(|error| format!("region slot {local_x},{local_z}: {error:?}"))?
            else {
                continue;
            };

            let external_bytes = if chunk.external {
                let parent = region_path.parent().unwrap_or_else(|| Path::new("."));
                let path = parent.join(format!("c.{}.{}.mcc", chunk.position.x, chunk.position.z));
                Some(read_bounded(&path, MAX_EXTERNAL_COMPRESSED_BYTES)?)
            } else {
                None
            };
            let external = external_bytes
                .as_deref()
                .map(|bytes| ExternalChunkPayload { bytes });
            let mut stored_chunk = importer
                .import_region_chunk(&region, local_x, local_z, external)
                .map_err(|error| {
                    format!(
                        "import failed for chunk {},{}: {error:?}",
                        chunk.position.x, chunk.position.z
                    )
                })?;
            stored_chunk
                .blocks
                .sections
                .sort_unstable_by_key(|section| section.section_y);

            for section in stored_chunk.blocks.sections {
                write!(
                    output,
                    "SECTION|{dimension}|{}|{}|{}|",
                    chunk.position.x, chunk.position.z, section.section_y
                )
                .map_err(|error| io_error(&error))?;
                for (index, state) in section.section.into_iter().enumerate() {
                    if index != 0 {
                        output.write_all(b",").map_err(|error| io_error(&error))?;
                    }
                    write!(output, "{}", state.as_usize()).map_err(|error| io_error(&error))?;
                }
                output.write_all(b"\n").map_err(|error| io_error(&error))?;
            }
        }
    }
    output.flush().map_err(|error| io_error(&error))?;
    Ok(())
}

fn usage(message: &str) -> String {
    format!("{message}; usage: r2c_import_section_facts <dimension> <r.x.z.mca> <output>")
}

fn validate_dimension(dimension: &str) -> Result<(), String> {
    if dimension.is_empty()
        || dimension
            .bytes()
            .any(|byte| matches!(byte, b'|' | b'\n' | b'\r'))
    {
        return Err("dimension is empty or contains a section-fact delimiter".to_owned());
    }
    Ok(())
}

fn parse_region_coordinates(path: &Path) -> Result<(i32, i32), String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("region path has no UTF-8 filename: {}", path.display()))?;
    let middle = name
        .strip_prefix("r.")
        .and_then(|name| name.strip_suffix(".mca"))
        .ok_or_else(|| format!("invalid Anvil region filename: {name}"))?;
    let mut parts = middle.split('.');
    let region_x = parts
        .next()
        .ok_or_else(|| format!("invalid Anvil region filename: {name}"))?
        .parse::<i32>()
        .map_err(|_| format!("invalid region X in filename: {name}"))?;
    let region_z = parts
        .next()
        .ok_or_else(|| format!("invalid Anvil region filename: {name}"))?
        .parse::<i32>()
        .map_err(|_| format!("invalid region Z in filename: {name}"))?;
    if parts.next().is_some() {
        return Err(format!("invalid Anvil region filename: {name}"));
    }
    Ok((region_x, region_z))
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file =
        File::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;
    let read_limit = u64::try_from(limit)
        .map_err(|_| "qualification file bound does not fit u64".to_owned())?
        .checked_add(1)
        .ok_or_else(|| "qualification file bound overflow".to_owned())?;
    let mut bytes = Vec::new();
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!(
            "{} exceeds qualification byte bound: actual>{limit}",
            path.display()
        ));
    }
    Ok(bytes)
}

fn io_error(error: &io::Error) -> String {
    format!("output write failed: {error}")
}

#[cfg(test)]
mod tests {
    use super::{parse_region_coordinates, validate_dimension};
    use std::path::Path;

    #[test]
    fn region_filename_coordinates_are_strict() {
        assert_eq!(
            parse_region_coordinates(Path::new("r.-2.7.mca")),
            Ok((-2, 7))
        );
        assert!(parse_region_coordinates(Path::new("r.2.7.mca.tmp")).is_err());
        assert!(parse_region_coordinates(Path::new("r.2.7.1.mca")).is_err());
    }

    #[test]
    fn dimension_cannot_inject_fact_lines() {
        assert!(validate_dimension("minecraft:overworld").is_ok());
        assert!(validate_dimension("minecraft:overworld\nSECTION|evil").is_err());
    }
}
