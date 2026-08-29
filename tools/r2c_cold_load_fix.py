#!/usr/bin/env python3
from pathlib import Path

# One-shot branch-local cleanup; removed after materialization.
PATH = Path("crates/qualification/helve-cold-load-qualification/src/lib.rs")
text = PATH.read_text(encoding="utf-8")

replacements = [
    (
        "    BlockProperty, BlockSectionDecodeScratch, BlockSectionScratchCapacities,\n",
        "    BlockSectionDecodeScratch, BlockSectionScratchCapacities,\n",
    ),
    (
        "        drop(chunk);\n",
        "",
    ),
    (
        "        ColdLoadHarness, DENSE_CELL_COPIES, DENSE_SECTION_COUNT, FIXTURE_COMPRESSED_BYTES,\n",
        "        BLOCK_SECTION_CELLS, ColdLoadHarness, DENSE_CELL_COPIES, DENSE_SECTION_COUNT,\n        FIXTURE_COMPRESSED_BYTES,\n",
    ),
]
for before, after in replacements:
    count = text.count(before)
    if count != 1:
        raise SystemExit(f"expected exactly one occurrence, found {count}: {before!r}")
    text = text.replace(before, after, 1)

PATH.write_text(text, encoding="utf-8")
