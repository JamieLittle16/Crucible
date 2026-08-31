//! Exact-target chunk-schema header inspection over decompressed NBT.
//!
//! This is intentionally a narrow schema cursor, not a Mojang-shaped chunk object. It establishes
//! target/version/coordinate/section-list framing before later section decoding constructs semantic
//! Helve state.

use helve_types::ChunkPos;

use crate::nbt::{NbtError, NbtLimits, NbtReader, TagType};

/// Minecraft 26.2 world `DataVersion` admitted by the existing vanilla-save qualification oracle.
pub const TARGET_DATA_VERSION_26_2: i32 = 4903;

/// Minimal validated stored-chunk identity needed before semantic section import.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoredChunkHeader {
    /// Exact target world data version.
    pub data_version: i32,
    /// Stored semantic chunk position.
    pub position: ChunkPos,
    /// Number of stored section compounds, including sections without block state payloads.
    pub stored_section_count: usize,
}

/// Fail-closed exact-target chunk-schema errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChunkImportError {
    /// Underlying bounded NBT framing failed.
    Nbt(NbtError),
    /// Required top-level chunk field is absent.
    MissingField { field: &'static str },
    /// Required top-level field appears more than once.
    DuplicateField { field: &'static str },
    /// A required field has the wrong NBT payload type.
    WrongTagType {
        field: &'static str,
        expected: TagType,
        actual: TagType,
    },
    /// Stored world data is not the exact pinned 26.2 schema.
    DataVersionMismatch { expected: i32, actual: i32 },
    /// Region framing and stored chunk coordinates disagree.
    PositionMismatch {
        expected: ChunkPos,
        actual: ChunkPos,
    },
    /// `sections` must be a homogeneous list of compound payloads.
    SectionListElementType { actual: TagType },
}

impl From<NbtError> for ChunkImportError {
    fn from(value: NbtError) -> Self {
        Self::Nbt(value)
    }
}

/// Inspects exact-target chunk identity/schema framing without allocating a generic NBT tree.
///
/// Unknown fields are skipped under `limits`. The section compounds themselves are validated as a
/// bounded list and skipped; semantic block/biome/light decoding belongs to the next importer layer.
///
/// # Errors
///
/// Returns an explicit error for malformed NBT, duplicate/missing required fields, wrong field types,
/// wrong target `DataVersion`, or disagreement with the expected Anvil region slot.
pub fn inspect_chunk_header(
    decompressed_nbt: &[u8],
    expected_position: ChunkPos,
    limits: NbtLimits,
) -> Result<StoredChunkHeader, ChunkImportError> {
    let mut reader = NbtReader::new(decompressed_nbt, limits);
    reader.begin_root_compound()?;

    let mut data_version = None;
    let mut x_pos = None;
    let mut z_pos = None;
    let mut section_count = None;

    while let Some(field) = reader.next_compound_field()? {
        match field.name {
            "DataVersion" => {
                require_type("DataVersion", field.tag_type, TagType::Int)?;
                set_once(&mut data_version, "DataVersion", reader.read_i32()?)?;
            }
            "xPos" => {
                require_type("xPos", field.tag_type, TagType::Int)?;
                set_once(&mut x_pos, "xPos", reader.read_i32()?)?;
            }
            "zPos" => {
                require_type("zPos", field.tag_type, TagType::Int)?;
                set_once(&mut z_pos, "zPos", reader.read_i32()?)?;
            }
            "sections" => {
                require_type("sections", field.tag_type, TagType::List)?;
                if section_count.is_some() {
                    return Err(ChunkImportError::DuplicateField { field: "sections" });
                }
                let header = reader.read_list_header()?;
                if header.element_type != TagType::Compound {
                    return Err(ChunkImportError::SectionListElementType {
                        actual: header.element_type,
                    });
                }
                for _ in 0..header.len {
                    reader.skip_payload(TagType::Compound)?;
                }
                section_count = Some(header.len);
            }
            _ => reader.skip_payload(field.tag_type)?,
        }
    }
    reader.finish_root()?;

    let data_version = required(data_version, "DataVersion")?;
    if data_version != TARGET_DATA_VERSION_26_2 {
        return Err(ChunkImportError::DataVersionMismatch {
            expected: TARGET_DATA_VERSION_26_2,
            actual: data_version,
        });
    }
    let actual_position = ChunkPos {
        x: required(x_pos, "xPos")?,
        z: required(z_pos, "zPos")?,
    };
    if actual_position != expected_position {
        return Err(ChunkImportError::PositionMismatch {
            expected: expected_position,
            actual: actual_position,
        });
    }

    Ok(StoredChunkHeader {
        data_version,
        position: actual_position,
        stored_section_count: required(section_count, "sections")?,
    })
}

fn require_type(
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

fn set_once<T>(
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

fn required<T>(slot: Option<T>, field: &'static str) -> Result<T, ChunkImportError> {
    slot.ok_or(ChunkImportError::MissingField { field })
}

#[cfg(test)]
mod tests {
    use super::{
        ChunkImportError, StoredChunkHeader, TARGET_DATA_VERSION_26_2, inspect_chunk_header,
    };
    use crate::nbt::{NbtLimits, TagType};
    use helve_types::ChunkPos;

    fn limits() -> NbtLimits {
        NbtLimits::new(256, 64, 4096, 16).expect("valid test limits")
    }

    fn name(bytes: &mut Vec<u8>, value: &str) {
        let length = u16::try_from(value.len()).expect("test name length fits u16");
        bytes.extend_from_slice(&length.to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn int_field(bytes: &mut Vec<u8>, field: &str, value: i32) {
        bytes.push(TagType::Int as u8);
        name(bytes, field);
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn chunk_nbt(position: ChunkPos, data_version: i32, sections: usize) -> Vec<u8> {
        let mut bytes = vec![TagType::Compound as u8, 0, 0];
        int_field(&mut bytes, "DataVersion", data_version);
        int_field(&mut bytes, "xPos", position.x);
        int_field(&mut bytes, "zPos", position.z);
        bytes.push(TagType::List as u8);
        name(&mut bytes, "sections");
        bytes.push(TagType::Compound as u8);
        bytes.extend_from_slice(
            &i32::try_from(sections)
                .expect("test section count fits i32")
                .to_be_bytes(),
        );
        for _ in 0..sections {
            bytes.push(TagType::End as u8);
        }
        bytes.push(TagType::End as u8);
        bytes
    }

    #[test]
    fn validates_exact_target_and_negative_region_coordinates() {
        let position = ChunkPos { x: -31, z: 65 };
        let bytes = chunk_nbt(position, TARGET_DATA_VERSION_26_2, 24);
        assert_eq!(
            inspect_chunk_header(&bytes, position, limits()),
            Ok(StoredChunkHeader {
                data_version: TARGET_DATA_VERSION_26_2,
                position,
                stored_section_count: 24,
            })
        );
    }

    #[test]
    fn unknown_nested_fields_are_skipped_without_generic_tree() {
        let position = ChunkPos { x: 4, z: -9 };
        let mut bytes = vec![TagType::Compound as u8, 0, 0];
        bytes.push(TagType::Compound as u8);
        name(&mut bytes, "unknown");
        int_field(&mut bytes, "nested", 7);
        bytes.push(TagType::End as u8);
        int_field(&mut bytes, "DataVersion", TARGET_DATA_VERSION_26_2);
        int_field(&mut bytes, "xPos", position.x);
        int_field(&mut bytes, "zPos", position.z);
        bytes.push(TagType::List as u8);
        name(&mut bytes, "sections");
        bytes.push(TagType::Compound as u8);
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        bytes.push(TagType::End as u8);

        let header = inspect_chunk_header(&bytes, position, limits()).expect("valid header");
        assert_eq!(header.stored_section_count, 0);
    }

    #[test]
    fn wrong_data_version_fails_closed() {
        let position = ChunkPos { x: 0, z: 0 };
        let bytes = chunk_nbt(position, TARGET_DATA_VERSION_26_2 - 1, 1);
        assert_eq!(
            inspect_chunk_header(&bytes, position, limits()),
            Err(ChunkImportError::DataVersionMismatch {
                expected: TARGET_DATA_VERSION_26_2,
                actual: TARGET_DATA_VERSION_26_2 - 1,
            })
        );
    }

    #[test]
    fn region_slot_position_mismatch_fails_closed() {
        let stored = ChunkPos { x: 3, z: 4 };
        let expected = ChunkPos { x: 4, z: 4 };
        let bytes = chunk_nbt(stored, TARGET_DATA_VERSION_26_2, 1);
        assert_eq!(
            inspect_chunk_header(&bytes, expected, limits()),
            Err(ChunkImportError::PositionMismatch {
                expected,
                actual: stored,
            })
        );
    }

    #[test]
    fn duplicate_required_field_is_rejected() {
        let position = ChunkPos { x: 0, z: 0 };
        let mut bytes = chunk_nbt(position, TARGET_DATA_VERSION_26_2, 1);
        let root_end = bytes.pop().expect("root end");
        int_field(&mut bytes, "xPos", position.x);
        bytes.push(root_end);
        assert_eq!(
            inspect_chunk_header(&bytes, position, limits()),
            Err(ChunkImportError::DuplicateField { field: "xPos" })
        );
    }

    #[test]
    fn sections_must_be_compound_list() {
        let position = ChunkPos { x: 0, z: 0 };
        let mut bytes = vec![TagType::Compound as u8, 0, 0];
        int_field(&mut bytes, "DataVersion", TARGET_DATA_VERSION_26_2);
        int_field(&mut bytes, "xPos", position.x);
        int_field(&mut bytes, "zPos", position.z);
        bytes.push(TagType::List as u8);
        name(&mut bytes, "sections");
        bytes.push(TagType::Int as u8);
        bytes.extend_from_slice(&0_i32.to_be_bytes());
        bytes.push(TagType::End as u8);
        assert_eq!(
            inspect_chunk_header(&bytes, position, limits()),
            Err(ChunkImportError::SectionListElementType {
                actual: TagType::Int,
            })
        );
    }
}
