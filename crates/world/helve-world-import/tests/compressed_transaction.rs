use helve_types::ChunkPos;
use helve_world_import::{
    BlockProperty, BlockSectionDecodeScratch, BlockStateResolver, ChunkCompression,
    ChunkPayloadLimits, DeflateChunkPayloadDecoder, ImportedBlockSectionBuilder, NbtLimits,
    RegionLimits, RegionView, StoredBlockImporter, StoredChunkSourceMetadata,
    anvil::{REGION_HEADER_BYTES, SECTOR_BYTES},
};

const REGION_LIMITS: RegionLimits = RegionLimits::new(64 * SECTOR_BYTES, 4 * SECTOR_BYTES);
const PAYLOAD_LIMITS: ChunkPayloadLimits = ChunkPayloadLimits::new(4 * SECTOR_BYTES, 64 * 1024);

// Exact Python-stdlib zlib/gzip encodings of the same 122-byte 26.2 NBT chunk fixture.
const ZLIB_CHUNK: &[u8] = &[
    0x78, 0x9c, 0x2d, 0x8c, 0x41, 0x0a, 0xc2, 0x30, 0x10, 0x45, 0x7f, 0x48, 0xa0, 0x4d,
    0x2a, 0x74, 0xe1, 0x21, 0xbc, 0x83, 0x6b, 0xd7, 0xe2, 0x4a, 0x70, 0x55, 0xa6, 0x61,
    0x0a, 0xc1, 0x36, 0x91, 0xce, 0x2c, 0x8a, 0xa7, 0x37, 0x95, 0xee, 0xfe, 0xe3, 0x3d,
    0x7e, 0x00, 0x2c, 0xba, 0x1b, 0x29, 0x3d, 0x79, 0x95, 0x54, 0x32, 0x70, 0xbe, 0x58,
    0xb8, 0xed, 0x51, 0x04, 0xd8, 0xa5, 0xfb, 0x1e, 0xd3, 0xa3, 0x15, 0x8e, 0x5a, 0x1b,
    0x09, 0x15, 0x8d, 0x81, 0x79, 0x21, 0xe0, 0x34, 0xce, 0x25, 0xbe, 0x07, 0x51, 0x52,
    0x16, 0x8f, 0xe6, 0x43, 0x33, 0xab, 0xf2, 0x3f, 0x69, 0xe1, 0xee, 0xb4, 0x30, 0xfa,
    0x25, 0x65, 0x8e, 0x2b, 0x4d, 0x7a, 0x15, 0x2d, 0x99, 0xf7, 0xbb, 0x1f, 0x2b, 0xac,
    0x1b, 0xcc,
];

const GZIP_CHUNK: &[u8] = &[
    0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x2d, 0x8c, 0x41, 0x0a,
    0xc2, 0x30, 0x10, 0x45, 0x7f, 0x48, 0xa0, 0x4d, 0x2a, 0x74, 0xe1, 0x21, 0xbc, 0x83,
    0x6b, 0xd7, 0xe2, 0x4a, 0x70, 0x55, 0xa6, 0x61, 0x0a, 0xc1, 0x36, 0x91, 0xce, 0x2c,
    0x8a, 0xa7, 0x37, 0x95, 0xee, 0xfe, 0xe3, 0x3d, 0x7e, 0x00, 0x2c, 0xba, 0x1b, 0x29,
    0x3d, 0x79, 0x95, 0x54, 0x32, 0x70, 0xbe, 0x58, 0xb8, 0xed, 0x51, 0x04, 0xd8, 0xa5,
    0xfb, 0x1e, 0xd3, 0xa3, 0x15, 0x8e, 0x5a, 0x1b, 0x09, 0x15, 0x8d, 0x81, 0x79, 0x21,
    0xe0, 0x34, 0xce, 0x25, 0xbe, 0x07, 0x51, 0x52, 0x16, 0x8f, 0xe6, 0x43, 0x33, 0xab,
    0xf2, 0x3f, 0x69, 0xe1, 0xee, 0xb4, 0x30, 0xfa, 0x25, 0x65, 0x8e, 0x2b, 0x4d, 0x7a,
    0x15, 0x2d, 0x99, 0xf7, 0xbb, 0x1f, 0xb4, 0x06, 0xe7, 0x58, 0x7a, 0x00, 0x00, 0x00,
];

struct Resolver;

impl BlockStateResolver for Resolver {
    type State = u16;

    fn resolve(&self, name: &str, properties: &[BlockProperty<'_>]) -> Option<Self::State> {
        match (name, properties) {
            ("minecraft:air", []) => Some(0),
            ("minecraft:stone", []) => Some(1),
            _ => None,
        }
    }
}

struct VecBuilder;

impl ImportedBlockSectionBuilder<u16> for VecBuilder {
    type Section = Vec<u16>;

    fn build_uniform(&mut self, state: u16) -> Self::Section {
        vec![state; 4096]
    }

    fn build_states(&mut self, states: &[u16]) -> Self::Section {
        states.to_vec()
    }
}

fn nbt_limits() -> NbtLimits {
    NbtLimits::new(256, 4096, 1024, 16).expect("valid test NBT limits")
}

fn set_location(bytes: &mut [u8], slot: usize, offset: u32, count: u8) {
    let raw = (offset << 8) | u32::from(count);
    bytes[slot * 4..slot * 4 + 4].copy_from_slice(&raw.to_be_bytes());
}

fn region_bytes(compression: u8, payload: &[u8], timestamp: u32) -> Vec<u8> {
    let mut bytes = vec![0_u8; REGION_HEADER_BYTES + SECTOR_BYTES];
    set_location(&mut bytes, 0, 2, 1);
    bytes[SECTOR_BYTES..SECTOR_BYTES + 4].copy_from_slice(&timestamp.to_be_bytes());

    let start = 2 * SECTOR_BYTES;
    let length = u32::try_from(payload.len() + 1).expect("test payload length fits u32");
    bytes[start..start + 4].copy_from_slice(&length.to_be_bytes());
    bytes[start + 4] = compression;
    bytes[start + 5..start + 5 + payload.len()].copy_from_slice(payload);
    bytes
}

fn import_compressed(
    decoder: &mut DeflateChunkPayloadDecoder,
    compression_id: u8,
    payload: &[u8],
    timestamp: u32,
) -> (Vec<u16>, StoredChunkSourceMetadata) {
    let bytes = region_bytes(compression_id, payload, timestamp);
    let region = RegionView::new(&bytes, 0, 0, REGION_LIMITS).expect("valid compressed region");
    let resolver = Resolver;
    let mut builder = VecBuilder;
    let mut scratch = BlockSectionDecodeScratch::new();
    let result = StoredBlockImporter::new(
        PAYLOAD_LIMITS,
        nbt_limits(),
        decoder,
        &resolver,
        &mut builder,
        &mut scratch,
    )
    .import_region_chunk(&region, 0, 0, None)
    .expect("compressed chunk import");

    assert_eq!(result.blocks.header.position, ChunkPos { x: 0, z: 0 });
    assert_eq!(result.blocks.header.data_version, 4903);
    assert_eq!(result.blocks.header.stored_section_count, 1);
    assert_eq!(result.blocks.sections.len(), 1);
    let section = result.blocks.sections.into_iter().next().expect("one section");
    (section.section, result.source)
}

#[test]
fn zlib_region_reaches_exact_semantic_section() {
    let mut decoder = DeflateChunkPayloadDecoder::try_with_output_limit(64 * 1024)
        .expect("bounded output scratch");
    let (states, source) = import_compressed(&mut decoder, 2, ZLIB_CHUNK, 17);

    assert_eq!(states.len(), 4096);
    assert!(states.iter().all(|&state| state == 1));
    assert_eq!(
        source,
        StoredChunkSourceMetadata {
            region_timestamp: 17,
            compression: ChunkCompression::Zlib,
            external: false,
        }
    );
}

#[test]
fn one_decoder_reuses_high_water_across_zlib_and_gzip_transactions() {
    let mut decoder = DeflateChunkPayloadDecoder::try_with_output_limit(64 * 1024)
        .expect("bounded output scratch");
    let initial_capacity = decoder.retained_output_capacity();

    let (zlib_states, zlib_source) = import_compressed(&mut decoder, 2, ZLIB_CHUNK, 23);
    let (gzip_states, gzip_source) = import_compressed(&mut decoder, 1, GZIP_CHUNK, 29);

    assert_eq!(zlib_states, gzip_states);
    assert!(gzip_states.iter().all(|&state| state == 1));
    assert_eq!(zlib_source.compression, ChunkCompression::Zlib);
    assert_eq!(gzip_source.compression, ChunkCompression::Gzip);
    assert_eq!(gzip_source.region_timestamp, 29);
    assert_eq!(decoder.retained_output_bytes(), 64 * 1024);
    assert_eq!(decoder.retained_output_capacity(), initial_capacity);
}
