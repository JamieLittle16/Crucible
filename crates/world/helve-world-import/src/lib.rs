//! Cold pregenerated-world import boundaries for Helve.
//!
//! This crate owns bounded parsing of persisted Minecraft world bytes. It deliberately does not own
//! live chunk residency, network projection, scheduling, persistence policy, or a generic NBT object
//! model. The import path should decode directly toward admitted semantic state.

#![forbid(unsafe_code)]

pub mod anvil;
pub mod chunk;
pub mod compression;
mod generated_state_lookup;
pub mod nbt;
pub mod state_lookup;
pub mod stored_blocks;
pub mod transaction;

pub use anvil::{ChunkCompression, RegionChunk, RegionError, RegionLimits, RegionView};
pub use chunk::{
    ChunkImportError, StoredChunkHeader, TARGET_DATA_VERSION_26_2, inspect_chunk_header,
};
pub use compression::{CompressedPayloadError, DeflateChunkPayloadDecoder, WrapperKind};
pub use nbt::{ListHeader, NamedTag, NbtError, NbtLimits, NbtReader, TagType};
pub use state_lookup::{
    Target262BlockStateResolver, canonical_state_fingerprint, resolve_target_26_2_block_state,
};
pub use stored_blocks::{
    BlockProperty, BlockSectionDecodeScratch, BlockSectionImportError,
    BlockSectionScratchCapacities, BlockStateResolver, ImportedBlockSection,
    ImportedBlockSectionBuilder, ImportedChunkBlocks, decode_chunk_block_sections,
};
pub use transaction::{
    ChunkPayloadDecoder, ChunkPayloadLimits, ExternalChunkPayload, ImportedStoredChunk,
    StoredBlockImporter, StoredChunkImportError, StoredChunkSourceMetadata,
    UncompressedChunkPayloadDecoder, UncompressedPayloadError,
};
