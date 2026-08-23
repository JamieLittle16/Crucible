//! Small semantic identities shared by Crucible contracts.
//!
//! This crate must remain boring. Storage topology, workers, packet formats, and implementation
//! details do not belong here.

#![forbid(unsafe_code)]

/// A block position in the semantic world coordinate system.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// A chunk position in the semantic world coordinate system.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ChunkPos {
    pub x: i32,
    pub z: i32,
}

/// Canonical logical simulation epoch.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TickEpoch(pub u64);

/// Distinguishes different live incarnations of the same semantic chunk position.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChunkGeneration(pub u64);

/// Monotone semantic revision inside one live chunk generation.
///
/// A same-state mutation does not advance this value. A real semantic mutation advances it exactly
/// once. Deferred work must pair the revision with [`ChunkGeneration`] before installing results.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ChunkRevision(pub u64);

/// Exact identity of one semantic state of a live chunk incarnation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ChunkStamp {
    pub generation: ChunkGeneration,
    pub revision: ChunkRevision,
}

/// Distinguishes reconstructed/replaced instances of a stateful engine component.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ComponentGeneration(pub u64);

#[cfg(test)]
mod tests {
    use super::{
        BlockPos, ChunkGeneration, ChunkPos, ChunkRevision, ChunkStamp, ComponentGeneration,
        TickEpoch,
    };

    #[test]
    fn semantic_ids_are_plain_values() {
        let block = BlockPos { x: 1, y: -2, z: 3 };
        let same_block = block;
        assert_eq!(block, same_block);

        let chunk = ChunkPos { x: -1, z: 4 };
        let same_chunk = chunk;
        assert_eq!(chunk, same_chunk);

        assert!(TickEpoch(4) > TickEpoch(3));
        assert_ne!(ChunkGeneration(1), ChunkGeneration(2));
        assert_ne!(ComponentGeneration(1), ComponentGeneration(2));
    }

    #[test]
    fn chunk_stamp_requires_generation_and_revision_identity() {
        let first = ChunkStamp {
            generation: ChunkGeneration(7),
            revision: ChunkRevision(11),
        };
        assert_ne!(
            first,
            ChunkStamp {
                generation: ChunkGeneration(8),
                revision: ChunkRevision(11),
            }
        );
        assert_ne!(
            first,
            ChunkStamp {
                generation: ChunkGeneration(7),
                revision: ChunkRevision(12),
            }
        );
    }
}
