//! Exact-target block-section decoding from decompressed chunk NBT.
//!
//! The decoder keeps persisted schema, semantic state resolution and live section representation
//! separate. It never constructs a generic NBT tree or a Mojang-shaped section object. Saved palette
//! identities are resolved once at this cold boundary, packed cells are decoded in the same YZX
//! linear order used by Helve's semantic section contract, and a statically selected builder creates
//! the caller's final section representation.

use helve_types::ChunkPos;

use crate::{
    chunk::{ChunkImportError, StoredChunkHeader, TARGET_DATA_VERSION_26_2},
    nbt::{NbtError, NbtLimits, NbtReader, TagType},
};

const BLOCK_SECTION_CELLS: usize = 4096;
const MIN_PACKED_BITS: usize = 4;
const PACKED_WORD_BITS: usize = u64::BITS as usize;
const FOUR_BIT_VALUES_PER_WORD: usize = PACKED_WORD_BITS / MIN_PACKED_BITS;
const FOUR_BIT_MASK: u64 = 0x0f;
const MAX_WRITTEN_PALETTE_ENTRIES: usize = BLOCK_SECTION_CELLS;
const HARD_MAX_PROPERTIES_PER_STATE: usize = 32;

/// One canonicalized saved block-state property.
///
/// Properties passed to [`BlockStateResolver`] are sorted lexicographically by name and duplicate
/// property names have already been rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockProperty<'a> {
    /// Property name from the saved palette entry.
    pub name: &'a str,
    /// Property value from the saved palette entry.
    pub value: &'a str,
}

/// Cold boundary that maps one saved target-version block-state identity into semantic state.
///
/// The importer intentionally does not prescribe the lookup mechanism. A generated descriptor,
/// sorted static table or another evidence-qualified cold lookup can implement this trait without
/// changing NBT decoding or live section storage.
pub trait BlockStateResolver {
    /// Compact semantic state identity emitted by the resolver.
    type State: Copy + Eq;

    /// Resolves one resource name plus canonicalized property set.
    ///
    /// Returning `None` rejects the saved state rather than fabricating or silently normalizing it.
    fn resolve(&self, name: &str, properties: &[BlockProperty<'_>]) -> Option<Self::State>;
}

/// Static construction boundary for the final live/reference block-section representation.
///
/// Non-uniform `states` always contains exactly 4096 semantic cells in YZX linear order. The slice
/// is backed by reusable importer scratch and must be consumed before the method returns.
pub trait ImportedBlockSectionBuilder<S: Copy + Eq> {
    /// Final section representation retained by the imported chunk.
    type Section;

    /// Constructs a homogeneous section without requiring a temporary 4096-cell fill.
    fn build_uniform(&mut self, state: S) -> Self::Section;

    /// Constructs a non-uniform section from exact semantic cell state.
    fn build_states(&mut self, states: &[S]) -> Self::Section;
}

/// Reusable cold decode storage shared across sections/chunks.
///
/// Capacity grows only when a larger observed palette/packed payload requires it. Clearing the
/// scratch between sections retains backing allocations so ordinary world loading does not allocate
/// one palette/word/cell vector per section.
#[derive(Debug)]
pub struct BlockSectionDecodeScratch<S: Copy + Eq> {
    palette: Vec<S>,
    packed_words: Vec<u64>,
    states: Vec<S>,
}

impl<S: Copy + Eq> BlockSectionDecodeScratch<S> {
    /// Creates empty reusable decode scratch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            palette: Vec::new(),
            packed_words: Vec::new(),
            states: Vec::new(),
        }
    }

    /// Current retained capacities for qualification/allocation accounting.
    #[must_use]
    pub fn capacities(&self) -> BlockSectionScratchCapacities {
        BlockSectionScratchCapacities {
            palette: self.palette.capacity(),
            packed_words: self.packed_words.capacity(),
            states: self.states.capacity(),
        }
    }

    fn clear_section(&mut self) {
        self.palette.clear();
        self.packed_words.clear();
        self.states.clear();
    }
}

impl<S: Copy + Eq> Default for BlockSectionDecodeScratch<S> {
    fn default() -> Self {
        Self::new()
    }
}

/// Retained scratch capacity witness used by importer qualification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockSectionScratchCapacities {
    /// Resolved semantic palette slots retained.
    pub palette: usize,
    /// Packed 64-bit source words retained.
    pub packed_words: usize,
    /// Decoded semantic cell slots retained.
    pub states: usize,
}

/// One final block-bearing stored section.
#[derive(Debug, Eq, PartialEq)]
pub struct ImportedBlockSection<Section> {
    /// Signed target section Y from the persisted section compound.
    pub section_y: i8,
    /// Caller-selected final/reference section representation.
    pub section: Section,
}

/// Exact-target block-state portion of one imported chunk.
#[derive(Debug, Eq, PartialEq)]
pub struct ImportedChunkBlocks<Section> {
    /// Validated target/version/position/stored-section identity.
    pub header: StoredChunkHeader,
    /// Final block-bearing sections in persisted list order.
    pub sections: Vec<ImportedBlockSection<Section>>,
}

/// Fail-closed semantic block-section decode errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockSectionImportError {
    /// Underlying bounded NBT framing failed.
    Nbt(NbtError),
    /// Top-level exact-target chunk identity/schema validation failed.
    Chunk(ChunkImportError),
    /// One section omitted a field required by the selected block import profile.
    SectionMissingField {
        /// Zero-based persisted section-list index.
        section_index: usize,
        /// Missing field name.
        field: &'static str,
    },
    /// One section repeated a singleton field.
    SectionDuplicateField {
        /// Zero-based persisted section-list index.
        section_index: usize,
        /// Duplicate field name.
        field: &'static str,
    },
    /// A section/block-state field has the wrong NBT type.
    SectionWrongTagType {
        /// Zero-based persisted section-list index.
        section_index: usize,
        /// Field name.
        field: &'static str,
        /// Required tag type.
        expected: TagType,
        /// Observed tag type.
        actual: TagType,
    },
    /// Two persisted section compounds claim the same signed section Y.
    DuplicateSectionY { section_y: i8 },
    /// `block_states.palette` must contain compound entries.
    PaletteListElementType {
        /// Zero-based persisted section-list index.
        section_index: usize,
        /// Observed element type.
        actual: TagType,
    },
    /// A block-state palette cannot be empty.
    EmptyPalette { section_index: usize },
    /// The selected writer-compatible profile rejects palettes larger than one section's cell count.
    PaletteTooLarge {
        /// Zero-based persisted section-list index.
        section_index: usize,
        /// Observed palette entries.
        actual: usize,
        /// Accepted profile bound.
        limit: usize,
    },
    /// A palette entry omitted its required resource name.
    PaletteEntryMissingName {
        section_index: usize,
        palette_index: usize,
    },
    /// A palette entry repeated `Name` or `Properties`.
    PaletteEntryDuplicateField {
        section_index: usize,
        palette_index: usize,
        field: &'static str,
    },
    /// A palette-entry field has the wrong NBT type.
    PaletteEntryWrongTagType {
        section_index: usize,
        palette_index: usize,
        field: &'static str,
        expected: TagType,
        actual: TagType,
    },
    /// One palette entry exceeded the fixed hostile-input property bound.
    TooManyProperties {
        section_index: usize,
        palette_index: usize,
        actual: usize,
        limit: usize,
    },
    /// A property value must be an NBT string.
    PropertyValueWrongTagType {
        section_index: usize,
        palette_index: usize,
        actual: TagType,
    },
    /// NBT compounds cannot contain the same property key twice.
    DuplicateProperty {
        section_index: usize,
        palette_index: usize,
    },
    /// The target-version semantic resolver does not recognize a saved palette identity.
    UnknownBlockState {
        section_index: usize,
        palette_index: usize,
    },
    /// A multi-state palette requires packed `data`.
    MissingPackedData { section_index: usize },
    /// A uniform palette may omit `data` or carry an empty long array only.
    UniformSectionHasPackedData { section_index: usize, words: usize },
    /// Packed long count disagrees with the non-spanning 26.2 palette law.
    PackedLongCount {
        section_index: usize,
        palette_entries: usize,
        bits_per_entry: usize,
        actual: usize,
        expected: usize,
    },
    /// One packed cell references an absent palette entry.
    PaletteIndexOutOfRange {
        section_index: usize,
        cell: usize,
        palette_index: usize,
        palette_entries: usize,
    },
    /// Checked representation arithmetic could not be expressed on this target.
    ArithmeticOverflow,
}

impl From<NbtError> for BlockSectionImportError {
    fn from(value: NbtError) -> Self {
        Self::Nbt(value)
    }
}

impl From<ChunkImportError> for BlockSectionImportError {
    fn from(value: ChunkImportError) -> Self {
        Self::Chunk(value)
    }
}

/// Decodes target 26.2 block sections in one schema-directed pass over decompressed chunk NBT.
///
/// Unknown chunk/section fields are bounded-skipped. Final section objects are built before this
/// function returns but are not installed into resident world authority here, so any late chunk
/// identity/version failure drops the uncommitted result transactionally.
///
/// The first selected persisted-world profile is deliberately writer-compatible: palettes larger
/// than 4096 entries are rejected even though a more permissive vanilla reader may accept unused
/// extra palette entries. Expanding that profile requires explicit evidence and requalification.
///
/// # Errors
///
/// Returns an explicit error for malformed/bounds-violating NBT, invalid target/version/coordinate
/// identity, duplicate/missing section schema, unresolved saved states, or invalid packed palette
/// data. Unsupported input is never silently normalized or dropped.
pub fn decode_chunk_block_sections<R, B>(
    decompressed_nbt: &[u8],
    expected_position: ChunkPos,
    limits: NbtLimits,
    resolver: &R,
    builder: &mut B,
    scratch: &mut BlockSectionDecodeScratch<R::State>,
) -> Result<ImportedChunkBlocks<B::Section>, BlockSectionImportError>
where
    R: BlockStateResolver,
    B: ImportedBlockSectionBuilder<R::State>,
{
    let mut reader = NbtReader::new(decompressed_nbt, limits);
    reader.begin_root_compound()?;

    let mut data_version = None;
    let mut x_pos = None;
    let mut z_pos = None;
    let mut stored_section_count = None;
    let mut decoded_sections = Vec::new();
    let mut seen_section_y = [false; 256];

    while let Some(field) = reader.next_compound_field()? {
        match field.name {
            "DataVersion" => {
                require_chunk_type("DataVersion", field.tag_type, TagType::Int)?;
                set_chunk_once(&mut data_version, "DataVersion", reader.read_i32()?)?;
            }
            "xPos" => {
                require_chunk_type("xPos", field.tag_type, TagType::Int)?;
                set_chunk_once(&mut x_pos, "xPos", reader.read_i32()?)?;
            }
            "zPos" => {
                require_chunk_type("zPos", field.tag_type, TagType::Int)?;
                set_chunk_once(&mut z_pos, "zPos", reader.read_i32()?)?;
            }
            "sections" => {
                require_chunk_type("sections", field.tag_type, TagType::List)?;
                if stored_section_count.is_some() {
                    return Err(ChunkImportError::DuplicateField { field: "sections" }.into());
                }
                let list = reader.read_list_header()?;
                if list.element_type != TagType::Compound {
                    return Err(ChunkImportError::SectionListElementType {
                        actual: list.element_type,
                    }
                    .into());
                }
                decoded_sections.reserve(list.len);
                for section_index in 0..list.len {
                    if let Some(section) = decode_section(
                        &mut reader,
                        section_index,
                        resolver,
                        builder,
                        scratch,
                        &mut seen_section_y,
                    )? {
                        decoded_sections.push(section);
                    }
                }
                stored_section_count = Some(list.len);
            }
            _ => reader.skip_payload(field.tag_type)?,
        }
    }
    reader.finish_root()?;

    let data_version = chunk_required(data_version, "DataVersion")?;
    if data_version != TARGET_DATA_VERSION_26_2 {
        return Err(ChunkImportError::DataVersionMismatch {
            expected: TARGET_DATA_VERSION_26_2,
            actual: data_version,
        }
        .into());
    }
    let actual_position = ChunkPos {
        x: chunk_required(x_pos, "xPos")?,
        z: chunk_required(z_pos, "zPos")?,
    };
    if actual_position != expected_position {
        return Err(ChunkImportError::PositionMismatch {
            expected: expected_position,
            actual: actual_position,
        }
        .into());
    }

    Ok(ImportedChunkBlocks {
        header: StoredChunkHeader {
            data_version,
            position: actual_position,
            stored_section_count: chunk_required(stored_section_count, "sections")?,
        },
        sections: decoded_sections,
    })
}

fn decode_section<R, B>(
    reader: &mut NbtReader<'_>,
    section_index: usize,
    resolver: &R,
    builder: &mut B,
    scratch: &mut BlockSectionDecodeScratch<R::State>,
    seen_section_y: &mut [bool; 256],
) -> Result<Option<ImportedBlockSection<B::Section>>, BlockSectionImportError>
where
    R: BlockStateResolver,
    B: ImportedBlockSectionBuilder<R::State>,
{
    let mut section_y = None;
    let mut block_section = None;
    let mut block_states_seen = false;

    while let Some(field) = reader.next_compound_field()? {
        match field.name {
            "Y" => {
                require_section_type(section_index, "Y", field.tag_type, TagType::Byte)?;
                set_section_once(&mut section_y, section_index, "Y", reader.read_i8()?)?;
            }
            "block_states" => {
                require_section_type(
                    section_index,
                    "block_states",
                    field.tag_type,
                    TagType::Compound,
                )?;
                if block_states_seen {
                    return Err(BlockSectionImportError::SectionDuplicateField {
                        section_index,
                        field: "block_states",
                    });
                }
                block_states_seen = true;
                block_section = Some(decode_block_states(
                    reader,
                    section_index,
                    resolver,
                    builder,
                    scratch,
                )?);
            }
            _ => reader.skip_payload(field.tag_type)?,
        }
    }

    let section_y = section_y.ok_or(BlockSectionImportError::SectionMissingField {
        section_index,
        field: "Y",
    })?;
    let seen_index = usize::from(section_y.to_ne_bytes()[0]);
    if seen_section_y[seen_index] {
        return Err(BlockSectionImportError::DuplicateSectionY { section_y });
    }
    seen_section_y[seen_index] = true;

    Ok(block_section.map(|section| ImportedBlockSection { section_y, section }))
}

fn decode_block_states<R, B>(
    reader: &mut NbtReader<'_>,
    section_index: usize,
    resolver: &R,
    builder: &mut B,
    scratch: &mut BlockSectionDecodeScratch<R::State>,
) -> Result<B::Section, BlockSectionImportError>
where
    R: BlockStateResolver,
    B: ImportedBlockSectionBuilder<R::State>,
{
    scratch.clear_section();
    let mut palette_seen = false;
    let mut data_seen = false;

    while let Some(field) = reader.next_compound_field()? {
        match field.name {
            "palette" => {
                require_section_type(
                    section_index,
                    "block_states.palette",
                    field.tag_type,
                    TagType::List,
                )?;
                if palette_seen {
                    return Err(BlockSectionImportError::SectionDuplicateField {
                        section_index,
                        field: "block_states.palette",
                    });
                }
                palette_seen = true;
                decode_palette(reader, section_index, resolver, &mut scratch.palette)?;
            }
            "data" => {
                require_section_type(
                    section_index,
                    "block_states.data",
                    field.tag_type,
                    TagType::LongArray,
                )?;
                if data_seen {
                    return Err(BlockSectionImportError::SectionDuplicateField {
                        section_index,
                        field: "block_states.data",
                    });
                }
                data_seen = true;
                let words = reader.read_long_array_len()?;
                scratch.packed_words.reserve(words);
                for _ in 0..words {
                    let signed = reader.read_i64()?;
                    scratch
                        .packed_words
                        .push(u64::from_be_bytes(signed.to_be_bytes()));
                }
            }
            _ => reader.skip_payload(field.tag_type)?,
        }
    }

    if !palette_seen {
        return Err(BlockSectionImportError::SectionMissingField {
            section_index,
            field: "block_states.palette",
        });
    }
    let palette_entries = scratch.palette.len();
    if palette_entries == 1 {
        if data_seen && !scratch.packed_words.is_empty() {
            return Err(BlockSectionImportError::UniformSectionHasPackedData {
                section_index,
                words: scratch.packed_words.len(),
            });
        }
        return Ok(builder.build_uniform(scratch.palette[0]));
    }
    if !data_seen {
        return Err(BlockSectionImportError::MissingPackedData { section_index });
    }

    let bits_per_entry = packed_bits(palette_entries)?;
    let values_per_word = PACKED_WORD_BITS / bits_per_entry;
    let expected_words = BLOCK_SECTION_CELLS.div_ceil(values_per_word);
    if scratch.packed_words.len() != expected_words {
        return Err(BlockSectionImportError::PackedLongCount {
            section_index,
            palette_entries,
            bits_per_entry,
            actual: scratch.packed_words.len(),
            expected: expected_words,
        });
    }

    decode_packed_states(
        section_index,
        &scratch.palette,
        &scratch.packed_words,
        bits_per_entry,
        &mut scratch.states,
    )?;
    debug_assert_eq!(scratch.states.len(), BLOCK_SECTION_CELLS);
    Ok(builder.build_states(&scratch.states))
}

fn decode_packed_states<S: Copy + Eq>(
    section_index: usize,
    palette: &[S],
    packed_words: &[u64],
    bits_per_entry: usize,
    states: &mut Vec<S>,
) -> Result<(), BlockSectionImportError> {
    states.reserve(BLOCK_SECTION_CELLS);

    if bits_per_entry == MIN_PACKED_BITS {
        for cell in 0..BLOCK_SECTION_CELLS {
            let word = packed_words[cell >> 4];
            let shift = (cell & (FOUR_BIT_VALUES_PER_WORD - 1)) << 2;
            let raw_palette_index = (word >> shift) & FOUR_BIT_MASK;
            let palette_index = usize::try_from(raw_palette_index)
                .map_err(|_| BlockSectionImportError::ArithmeticOverflow)?;
            let state = palette.get(palette_index).copied().ok_or(
                BlockSectionImportError::PaletteIndexOutOfRange {
                    section_index,
                    cell,
                    palette_index,
                    palette_entries: palette.len(),
                },
            )?;
            states.push(state);
        }
        return Ok(());
    }

    let values_per_word = PACKED_WORD_BITS / bits_per_entry;
    let mask = (1_u64 << bits_per_entry) - 1;
    for cell in 0..BLOCK_SECTION_CELLS {
        let word = packed_words[cell / values_per_word];
        let shift = (cell % values_per_word) * bits_per_entry;
        let raw_palette_index = (word >> shift) & mask;
        let palette_index = usize::try_from(raw_palette_index)
            .map_err(|_| BlockSectionImportError::ArithmeticOverflow)?;
        let state = palette.get(palette_index).copied().ok_or(
            BlockSectionImportError::PaletteIndexOutOfRange {
                section_index,
                cell,
                palette_index,
                palette_entries: palette.len(),
            },
        )?;
        states.push(state);
    }
    Ok(())
}

fn decode_palette<R>(
    reader: &mut NbtReader<'_>,
    section_index: usize,
    resolver: &R,
    palette: &mut Vec<R::State>,
) -> Result<(), BlockSectionImportError>
where
    R: BlockStateResolver,
{
    let list = reader.read_list_header()?;
    if list.element_type != TagType::Compound {
        return Err(BlockSectionImportError::PaletteListElementType {
            section_index,
            actual: list.element_type,
        });
    }
    if list.len == 0 {
        return Err(BlockSectionImportError::EmptyPalette { section_index });
    }
    if list.len > MAX_WRITTEN_PALETTE_ENTRIES {
        return Err(BlockSectionImportError::PaletteTooLarge {
            section_index,
            actual: list.len,
            limit: MAX_WRITTEN_PALETTE_ENTRIES,
        });
    }
    palette.reserve(list.len);
    for palette_index in 0..list.len {
        palette.push(decode_palette_entry(
            reader,
            section_index,
            palette_index,
            resolver,
        )?);
    }
    Ok(())
}

fn decode_palette_entry<R>(
    reader: &mut NbtReader<'_>,
    section_index: usize,
    palette_index: usize,
    resolver: &R,
) -> Result<R::State, BlockSectionImportError>
where
    R: BlockStateResolver,
{
    let mut name = None;
    let mut properties_seen = false;
    let empty = BlockProperty {
        name: "",
        value: "",
    };
    let mut properties = [empty; HARD_MAX_PROPERTIES_PER_STATE];
    let mut property_count = 0_usize;

    while let Some(field) = reader.next_compound_field()? {
        match field.name {
            "Name" => {
                if field.tag_type != TagType::String {
                    return Err(BlockSectionImportError::PaletteEntryWrongTagType {
                        section_index,
                        palette_index,
                        field: "Name",
                        expected: TagType::String,
                        actual: field.tag_type,
                    });
                }
                if name.is_some() {
                    return Err(BlockSectionImportError::PaletteEntryDuplicateField {
                        section_index,
                        palette_index,
                        field: "Name",
                    });
                }
                name = Some(reader.read_string()?);
            }
            "Properties" => {
                if field.tag_type != TagType::Compound {
                    return Err(BlockSectionImportError::PaletteEntryWrongTagType {
                        section_index,
                        palette_index,
                        field: "Properties",
                        expected: TagType::Compound,
                        actual: field.tag_type,
                    });
                }
                if properties_seen {
                    return Err(BlockSectionImportError::PaletteEntryDuplicateField {
                        section_index,
                        palette_index,
                        field: "Properties",
                    });
                }
                properties_seen = true;
                while let Some(property) = reader.next_compound_field()? {
                    if property.tag_type != TagType::String {
                        return Err(BlockSectionImportError::PropertyValueWrongTagType {
                            section_index,
                            palette_index,
                            actual: property.tag_type,
                        });
                    }
                    if property_count == HARD_MAX_PROPERTIES_PER_STATE {
                        return Err(BlockSectionImportError::TooManyProperties {
                            section_index,
                            palette_index,
                            actual: property_count + 1,
                            limit: HARD_MAX_PROPERTIES_PER_STATE,
                        });
                    }
                    properties[property_count] = BlockProperty {
                        name: property.name,
                        value: reader.read_string()?,
                    };
                    property_count += 1;
                }
            }
            _ => reader.skip_payload(field.tag_type)?,
        }
    }

    let name = name.ok_or(BlockSectionImportError::PaletteEntryMissingName {
        section_index,
        palette_index,
    })?;
    let properties = &mut properties[..property_count];
    properties.sort_unstable_by(|left, right| {
        left.name
            .cmp(right.name)
            .then_with(|| left.value.cmp(right.value))
    });
    if properties
        .windows(2)
        .any(|pair| pair[0].name == pair[1].name)
    {
        return Err(BlockSectionImportError::DuplicateProperty {
            section_index,
            palette_index,
        });
    }

    resolver
        .resolve(name, properties)
        .ok_or(BlockSectionImportError::UnknownBlockState {
            section_index,
            palette_index,
        })
}

fn packed_bits(palette_entries: usize) -> Result<usize, BlockSectionImportError> {
    debug_assert!(palette_entries > 1);
    let value = palette_entries - 1;
    let width_u32 = usize::BITS - value.leading_zeros();
    let width =
        usize::try_from(width_u32).map_err(|_| BlockSectionImportError::ArithmeticOverflow)?;
    Ok(width.max(MIN_PACKED_BITS))
}

fn require_chunk_type(
    field: &'static str,
    actual: TagType,
    expected: TagType,
) -> Result<(), ChunkImportError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ChunkImportError::WrongTagType {
            field,
            expected,
            actual,
        })
    }
}

fn set_chunk_once<T>(
    slot: &mut Option<T>,
    field: &'static str,
    value: T,
) -> Result<(), ChunkImportError> {
    if slot.is_some() {
        Err(ChunkImportError::DuplicateField { field })
    } else {
        *slot = Some(value);
        Ok(())
    }
}

fn chunk_required<T>(slot: Option<T>, field: &'static str) -> Result<T, ChunkImportError> {
    slot.ok_or(ChunkImportError::MissingField { field })
}

fn require_section_type(
    section_index: usize,
    field: &'static str,
    actual: TagType,
    expected: TagType,
) -> Result<(), BlockSectionImportError> {
    if actual == expected {
        Ok(())
    } else {
        Err(BlockSectionImportError::SectionWrongTagType {
            section_index,
            field,
            expected,
            actual,
        })
    }
}

fn set_section_once<T>(
    slot: &mut Option<T>,
    section_index: usize,
    field: &'static str,
    value: T,
) -> Result<(), BlockSectionImportError> {
    if slot.is_some() {
        Err(BlockSectionImportError::SectionDuplicateField {
            section_index,
            field,
        })
    } else {
        *slot = Some(value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BLOCK_SECTION_CELLS, BlockProperty, BlockSectionDecodeScratch, BlockSectionImportError,
        BlockStateResolver, ImportedBlockSection, ImportedBlockSectionBuilder, ImportedChunkBlocks,
        decode_chunk_block_sections,
    };
    use crate::{
        chunk::{ChunkImportError, StoredChunkHeader, TARGET_DATA_VERSION_26_2},
        nbt::{NbtLimits, TagType},
    };
    use helve_types::ChunkPos;

    #[derive(Default)]
    struct VecBuilder;

    impl ImportedBlockSectionBuilder<u16> for VecBuilder {
        type Section = Vec<u16>;

        fn build_uniform(&mut self, state: u16) -> Self::Section {
            vec![state; BLOCK_SECTION_CELLS]
        }

        fn build_states(&mut self, states: &[u16]) -> Self::Section {
            states.to_vec()
        }
    }

    struct Resolver;

    impl BlockStateResolver for Resolver {
        type State = u16;

        fn resolve(&self, name: &str, properties: &[BlockProperty<'_>]) -> Option<Self::State> {
            match (name, properties) {
                ("minecraft:air", []) => Some(0),
                ("minecraft:stone", []) => Some(1),
                (
                    "minecraft:oak_log",
                    [
                        BlockProperty {
                            name: "axis",
                            value: "x",
                        },
                    ],
                ) => Some(2),
                (
                    "minecraft:test",
                    [
                        BlockProperty {
                            name: "a",
                            value: "1",
                        },
                        BlockProperty {
                            name: "z",
                            value: "2",
                        },
                    ],
                ) => Some(3),
                _ => None,
            }
        }
    }

    fn limits() -> NbtLimits {
        NbtLimits::new(256, 4096, 1024, 16).expect("valid test limits")
    }

    fn name(bytes: &mut Vec<u8>, value: &str) {
        let length = u16::try_from(value.len()).expect("test name fits u16");
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn named_header(bytes: &mut Vec<u8>, tag_type: TagType, field: &str) {
        bytes.push(tag_type as u8);
        name(bytes, field);
    }

    fn int_field(bytes: &mut Vec<u8>, field: &str, value: i32) {
        named_header(bytes, TagType::Int, field);
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn byte_field(bytes: &mut Vec<u8>, field: &str, value: i8) {
        named_header(bytes, TagType::Byte, field);
        bytes.push(value.to_ne_bytes()[0]);
    }

    fn string_payload(bytes: &mut Vec<u8>, value: &str) {
        name(bytes, value);
    }

    fn palette_entry(
        block: &str,
        properties: &[(&str, &str)],
        reverse_properties: bool,
    ) -> Vec<u8> {
        let mut bytes = Vec::new();
        named_header(&mut bytes, TagType::String, "Name");
        string_payload(&mut bytes, block);
        if !properties.is_empty() {
            named_header(&mut bytes, TagType::Compound, "Properties");
            if reverse_properties {
                for &(key, value) in properties.iter().rev() {
                    named_header(&mut bytes, TagType::String, key);
                    string_payload(&mut bytes, value);
                }
            } else {
                for &(key, value) in properties {
                    named_header(&mut bytes, TagType::String, key);
                    string_payload(&mut bytes, value);
                }
            }
            bytes.push(TagType::End as u8);
        }
        bytes.push(TagType::End as u8);
        bytes
    }

    fn packed_words(indices: &[usize], palette_entries: usize) -> Vec<i64> {
        let bits = super::packed_bits(palette_entries).expect("test palette bits");
        let values_per_word = 64 / bits;
        let mut words = vec![0_u64; indices.len().div_ceil(values_per_word)];
        for (cell, &index) in indices.iter().enumerate() {
            let index = u64::try_from(index).expect("test palette index fits u64");
            words[cell / values_per_word] |= index << ((cell % values_per_word) * bits);
        }
        words
            .into_iter()
            .map(|word| i64::from_be_bytes(word.to_be_bytes()))
            .collect()
    }

    fn block_states_payload(entries: &[Vec<u8>], indices: Option<&[usize]>) -> Vec<u8> {
        let mut bytes = Vec::new();
        named_header(&mut bytes, TagType::List, "palette");
        bytes.push(TagType::Compound as u8);
        bytes.extend_from_slice(
            &i32::try_from(entries.len())
                .expect("test palette length fits i32")
                .to_be_bytes(),
        );
        for entry in entries {
            bytes.extend_from_slice(entry);
        }
        if let Some(indices) = indices {
            named_header(&mut bytes, TagType::LongArray, "data");
            let words = packed_words(indices, entries.len());
            bytes.extend_from_slice(
                &i32::try_from(words.len())
                    .expect("test word count fits i32")
                    .to_be_bytes(),
            );
            for word in words {
                bytes.extend_from_slice(&word.to_be_bytes());
            }
        }
        bytes.push(TagType::End as u8);
        bytes
    }

    fn section_payload(y: i8, block_states: Option<Vec<u8>>) -> Vec<u8> {
        let mut bytes = Vec::new();
        byte_field(&mut bytes, "Y", y);
        if let Some(block_states) = block_states {
            named_header(&mut bytes, TagType::Compound, "block_states");
            bytes.extend_from_slice(&block_states);
        }
        bytes.push(TagType::End as u8);
        bytes
    }

    fn chunk_nbt(position: ChunkPos, sections: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = vec![TagType::Compound as u8, 0, 0];
        int_field(&mut bytes, "DataVersion", TARGET_DATA_VERSION_26_2);
        int_field(&mut bytes, "xPos", position.x);
        int_field(&mut bytes, "zPos", position.z);
        named_header(&mut bytes, TagType::List, "sections");
        bytes.push(TagType::Compound as u8);
        bytes.extend_from_slice(
            &i32::try_from(sections.len())
                .expect("test section count fits i32")
                .to_be_bytes(),
        );
        for section in sections {
            bytes.extend_from_slice(section);
        }
        bytes.push(TagType::End as u8);
        bytes
    }

    fn decode(
        bytes: &[u8],
        position: ChunkPos,
    ) -> Result<ImportedChunkBlocks<Vec<u16>>, BlockSectionImportError> {
        decode_chunk_block_sections(
            bytes,
            position,
            limits(),
            &Resolver,
            &mut VecBuilder,
            &mut BlockSectionDecodeScratch::new(),
        )
    }

    #[test]
    fn uniform_palette_builds_without_packed_cell_scratch() {
        let position = ChunkPos { x: -31, z: 65 };
        let section = section_payload(
            -4,
            Some(block_states_payload(
                &[palette_entry("minecraft:air", &[], false)],
                None,
            )),
        );
        let bytes = chunk_nbt(position, &[section]);
        let mut scratch = BlockSectionDecodeScratch::new();
        let result = decode_chunk_block_sections(
            &bytes,
            position,
            limits(),
            &Resolver,
            &mut VecBuilder,
            &mut scratch,
        )
        .expect("valid uniform chunk");
        assert_eq!(
            result.header,
            StoredChunkHeader {
                data_version: TARGET_DATA_VERSION_26_2,
                position,
                stored_section_count: 1,
            }
        );
        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].section_y, -4);
        assert!(result.sections[0].section.iter().all(|&state| state == 0));
        assert_eq!(scratch.capacities().states, 0);
    }

    #[test]
    fn non_spanning_packed_palette_decodes_exact_yzx_linear_cells() {
        let position = ChunkPos { x: 0, z: 0 };
        let indices: Vec<usize> = (0..BLOCK_SECTION_CELLS).map(|cell| cell % 3).collect();
        let section = section_payload(
            0,
            Some(block_states_payload(
                &[
                    palette_entry("minecraft:air", &[], false),
                    palette_entry("minecraft:stone", &[], false),
                    palette_entry("minecraft:oak_log", &[("axis", "x")], false),
                ],
                Some(&indices),
            )),
        );
        let result =
            decode(&chunk_nbt(position, &[section]), position).expect("valid packed chunk");
        assert_eq!(result.sections[0].section.len(), BLOCK_SECTION_CELLS);
        for (cell, &state) in result.sections[0].section.iter().enumerate() {
            assert_eq!(usize::from(state), cell % 3);
        }
    }

    #[test]
    fn five_bit_generic_fallback_decodes_non_spanning_boundaries() {
        let palette: Vec<u16> = (0_u16..17).collect();
        let indices: Vec<usize> = (0..BLOCK_SECTION_CELLS).map(|cell| cell % 17).collect();
        let packed: Vec<u64> = packed_words(&indices, palette.len())
            .into_iter()
            .map(|word| u64::from_be_bytes(word.to_be_bytes()))
            .collect();
        let bits = super::packed_bits(palette.len()).expect("five-bit palette width");
        assert_eq!(bits, 5);

        let mut states = Vec::new();
        super::decode_packed_states(7, &palette, &packed, bits, &mut states)
            .expect("valid five-bit fallback");
        assert_eq!(states.len(), BLOCK_SECTION_CELLS);
        for (cell, &state) in states.iter().enumerate() {
            assert_eq!(usize::from(state), cell % 17);
        }
    }

    #[test]
    fn palette_properties_are_canonicalized_without_string_concatenation() {
        let position = ChunkPos { x: 1, z: -1 };
        let section = section_payload(
            3,
            Some(block_states_payload(
                &[palette_entry(
                    "minecraft:test",
                    &[("a", "1"), ("z", "2")],
                    true,
                )],
                None,
            )),
        );
        let result = decode(&chunk_nbt(position, &[section]), position).expect("resolved state");
        assert!(result.sections[0].section.iter().all(|&state| state == 3));
    }

    #[test]
    fn sections_without_block_states_remain_in_header_but_emit_no_block_section() {
        let position = ChunkPos { x: 2, z: 3 };
        let empty = section_payload(-5, None);
        let block = section_payload(
            -4,
            Some(block_states_payload(
                &[palette_entry("minecraft:stone", &[], false)],
                None,
            )),
        );
        let result = decode(&chunk_nbt(position, &[empty, block]), position).expect("valid chunk");
        assert_eq!(result.header.stored_section_count, 2);
        assert_eq!(result.sections.len(), 1);
        assert_eq!(result.sections[0].section_y, -4);
    }

    #[test]
    fn duplicate_section_y_fails_closed_even_when_one_section_has_no_blocks() {
        let position = ChunkPos { x: 0, z: 0 };
        let sections = [section_payload(0, None), section_payload(0, None)];
        assert_eq!(
            decode(&chunk_nbt(position, &sections), position),
            Err(BlockSectionImportError::DuplicateSectionY { section_y: 0 })
        );
    }

    #[test]
    fn multi_state_palette_requires_exact_packed_word_count() {
        let position = ChunkPos { x: 0, z: 0 };
        let entries = [
            palette_entry("minecraft:air", &[], false),
            palette_entry("minecraft:stone", &[], false),
        ];
        let indices = vec![0; BLOCK_SECTION_CELLS];
        let mut block_states = block_states_payload(&entries, Some(&indices));
        let remove_start = block_states.len() - 1 - 8;
        block_states.drain(remove_start..remove_start + 8);
        let word_count_offset = entries.iter().map(Vec::len).sum::<usize>()
            + 1
            + 2
            + "palette".len()
            + 1
            + 4
            + 1
            + 2
            + "data".len();
        let expected_words = BLOCK_SECTION_CELLS.div_ceil(64 / 4);
        let short_words = i32::try_from(expected_words - 1).expect("bounded");
        block_states[word_count_offset..word_count_offset + 4]
            .copy_from_slice(&short_words.to_be_bytes());
        let section = section_payload(0, Some(block_states));
        assert!(matches!(
            decode(&chunk_nbt(position, &[section]), position),
            Err(BlockSectionImportError::PackedLongCount { .. })
        ));
    }

    #[test]
    fn out_of_range_packed_palette_index_is_rejected() {
        let position = ChunkPos { x: 0, z: 0 };
        let entries = [
            palette_entry("minecraft:air", &[], false),
            palette_entry("minecraft:stone", &[], false),
        ];
        let mut indices = vec![0; BLOCK_SECTION_CELLS];
        indices[17] = 2;
        let section = section_payload(0, Some(block_states_payload(&entries, Some(&indices))));
        assert_eq!(
            decode(&chunk_nbt(position, &[section]), position),
            Err(BlockSectionImportError::PaletteIndexOutOfRange {
                section_index: 0,
                cell: 17,
                palette_index: 2,
                palette_entries: 2,
            })
        );
    }

    #[test]
    fn unknown_saved_state_is_rejected() {
        let position = ChunkPos { x: 0, z: 0 };
        let section = section_payload(
            0,
            Some(block_states_payload(
                &[palette_entry("minecraft:not_in_target", &[], false)],
                None,
            )),
        );
        assert_eq!(
            decode(&chunk_nbt(position, &[section]), position),
            Err(BlockSectionImportError::UnknownBlockState {
                section_index: 0,
                palette_index: 0,
            })
        );
    }

    #[test]
    fn exact_target_position_validation_remains_transactional() {
        let stored = ChunkPos { x: 4, z: 5 };
        let expected = ChunkPos { x: 5, z: 5 };
        let section = section_payload(
            0,
            Some(block_states_payload(
                &[palette_entry("minecraft:air", &[], false)],
                None,
            )),
        );
        assert_eq!(
            decode(&chunk_nbt(stored, &[section]), expected),
            Err(BlockSectionImportError::Chunk(
                ChunkImportError::PositionMismatch {
                    expected,
                    actual: stored,
                }
            ))
        );
    }

    #[test]
    fn result_shape_is_final_section_objects_not_generic_nbt_nodes() {
        let position = ChunkPos { x: 0, z: 0 };
        let section = section_payload(
            1,
            Some(block_states_payload(
                &[palette_entry("minecraft:stone", &[], false)],
                None,
            )),
        );
        let result = decode(&chunk_nbt(position, &[section]), position).expect("valid chunk");
        assert_eq!(
            result.sections,
            vec![ImportedBlockSection {
                section_y: 1,
                section: vec![1; BLOCK_SECTION_CELLS],
            }]
        );
    }
}
