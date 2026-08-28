#!/usr/bin/env python3
from __future__ import annotations

import re
from pathlib import Path

SOURCE = Path("crates/world/helve-world-import/src/compression.rs")
DOC = Path("docs/qualification/R2C_PREGENERATED_WORLD_IMPORT_QUALIFICATION.md")


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if text.count(old) != 1:
        raise SystemExit(f"{label} anchor changed")
    return text.replace(old, new)


def replace_section(text: str, start: str, end: str, replacement: str, label: str) -> str:
    pattern = re.compile(re.escape(start) + r".*?" + re.escape(end), re.S)
    updated, count = pattern.subn(replacement + end, text, count=1)
    if count != 1:
        raise SystemExit(f"{label} section anchor changed")
    return updated


def main() -> None:
    source = SOURCE.read_text(encoding="utf-8")
    source = replace_once(
        source,
        "The decoder retains one initialized output buffer at its historical high-water mark and one\n/// allocation-free DEFLATE state.",
        "The decoder retains one initialized output buffer at the caller-selected decompressed bound and\n/// one allocation-free DEFLATE state.",
        "compression source wording",
    )
    SOURCE.write_text(source, encoding="utf-8")

    text = DOC.read_text(encoding="utf-8")
    text = replace_once(
        text,
        "Status: **R2C.3 bounded framing + semantic stored-block transaction qualified; compressed codec, full target resolver, real-save differential and resident install pending**",
        "Status: **R2C.3 bounded framing + exact 26.2 state resolver + bounded gzip/zlib semantic transaction qualified; real-save differential, resident install and whole-import performance decision pending**",
        "qualification status",
    )
    text = replace_once(
        text,
        """The dependency-free `UncompressedChunkPayloadDecoder` is currently the only admitted Rust production
mechanism. It returns uncompressed payloads zero-copy and enforces the decompressed-byte limit.
Gzip, zlib and LZ4 fail closed under that decoder.

Synthetic qualification covers both inline and separately supplied external uncompressed records. A
static test decoder also proves that compression mechanism selection does not alter the semantic
transaction shape.""",
        """`UncompressedChunkPayloadDecoder` remains the zero-copy reference mechanism for compression ID 3.
The qualified `DeflateChunkPayloadDecoder` candidate admits uncompressed, zlib and gzip through the
same static transaction boundary; LZ4 remains fail-closed. Its compressed path owns one fallibly
allocated reusable output buffer sized to the caller-selected decompressed bound and reuses one
allocation-free DEFLATE state between chunks. Once that buffer is initialized, a chunk decode performs
no codec-side allocation or runtime decoder construction.

Synthetic qualification covers external uncompressed records plus complete inline zlib and gzip
region -> decompression -> NBT -> semantic-section transactions. The same decoder instance is reused
across both compressed wrappers without changing semantic output. Existing static decoder tests prove
that mechanism selection does not alter the transaction shape.""",
        "transaction qualification",
    )

    codec = """## Compressed-codec admission and hermeticity

Gzip/zlib now have a qualified first production candidate without weakening Helve's hermetic build
boundary. The exact reviewed dependency surface is:

- `miniz_oxide 0.8.9`, `default-features = false`;
- `adler2 2.0.1` as its checksum dependency;
- repository-owned Cargo vendor sources selected through `.cargo/config.toml`;
- exact versions/checksums frozen by `Cargo.lock` and the dependency allowlist;
- no git dependency, SIMD checksum feature or codec-side allocator feature.

The production wrapper calls the safe allocation-free `DecompressorOxide` core directly instead of
miniz's vector or generic streaming convenience APIs. This avoids a per-loader streaming dictionary
and writes decompressed bytes directly into one Helve-owned reusable output slice. The current
single-pass candidate sizes that initialized slice to the caller-selected decompressed bound. An
adaptive smaller retained scratch policy is a future mechanism candidate and must win on whole-import
CPU + memory evidence before replacing this simpler baseline.

Zlib admission requires exact stream consumption and the wrapper's Adler-32 check. Helve owns gzip
framing and validates magic, DEFLATE method, reserved flags, bounded optional extra/name/comment
fields, optional header CRC16, exact raw-DEFLATE consumption, CRC32 and ISIZE. Concatenated members or
hidden trailing compressed bytes are not silently admitted. Output exhaustion fails as a hard
`max_decompressed_bytes` violation, providing a decompression-bomb boundary before NBT parsing.

Hermeticity is permanent qualification, not a one-shot setup fact. The dedicated import workflow
creates a fresh empty `CARGO_HOME` and performs `cargo check` plus `cargo test` with
`--offline --locked`; therefore a missing vendor source or accidental registry dependency fails before
normal cached qualification can mask it. The repository guard and dependency allowlist also remain
active. Existing Helve Cargo aliases are preserved alongside the vendor source replacement.

Codec qualification currently establishes:

- maximum compressed-size enforcement in Anvil/external framing;
- hard decompressed-size enforcement in the decoder;
- exact EOF/trailing-data policy;
- zlib Adler-32 and gzip header/CRC32/ISIZE policy;
- reusable output-state behaviour;
- malformed/truncated/checksum/unsupported-wrapper regressions;
- end-to-end zlib/gzip semantic import on deterministic 26.2 NBT fixtures.

LZ4 remains explicitly unadmitted. Real-save differential evidence and whole-import target-hardware
cost still remain before R2C.3 is considered fully production-selected.

"""
    text = replace_section(
        text,
        "## Compressed-codec admission and hermeticity\n",
        "## Target block-state resolver",
        codec,
        "codec admission",
    )

    resolver = """## Target block-state resolver

The exact Minecraft Java 26.2 persisted-state resolver is qualified. It is generated from the same
source/runtime-qualified state dataset that defines Helve's dense vanilla-identity `BlockStateId`
universe and therefore does not invent a second numbering scheme.

The generated cold index covers all 32,366 admitted states in 416,665 bytes. Lookup hashes the already
canonicalized saved name/properties without constructing a canonical key string, maps to one generated
candidate, and then performs exact structured name/property verification before returning the existing
`BlockStateId`. Hash equality is indexing only and never semantic authority on hostile input.

The committed table is reproducible from regenerated official 26.2 runtime identities bound to the
frozen source qualification. The dedicated lookup workflow regenerates and byte-verifies the exact
artifact before Rust qualification. Keep this cold index separate from HOT mutation/state-fact tables
so persisted-world compatibility adds no ordinary block-access tax.

"""
    text = replace_section(
        text,
        "## Target block-state resolver\n",
        "## Differential qualification",
        resolver,
        "target resolver",
    )
    text = replace_once(
        text,
        "Once the full generated target resolver and an\nadmitted gzip/zlib mechanism exist, the permanent gate feeds the same pinned 26.2 region corpus to:",
        "With the generated target resolver and gzip/zlib mechanism now qualified, the next permanent gate\nfeeds the same pinned 26.2 region corpus to:",
        "differential next-step",
    )
    DOC.write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
