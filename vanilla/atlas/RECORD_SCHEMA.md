# Vanilla Source Review Record v1

Records live under `vanilla/records/**/*.json` and are authoritative machine-readable review metadata.

Example:

```json
{
  "schema": 1,
  "id": "VAR-WORLD-SECTION-001",
  "status": "VAR_REVIEWED",
  "source": {
    "type": "net.minecraft.world.level.chunk.LevelChunkSection",
    "signature": "setBlockState(...) ",
    "fingerprint_algorithm": "java-token-v2-literal-sensitive",
    "normalized_sha256": "...",
    "body_sha256": "..."
  },
  "classifications": ["SEMANTIC_GAMEPLAY"],
  "hazards_reviewed": ["ORDERING"],
  "semantic_rules": ["SEM-WORLD-SECTION-001"],
  "evidence": ["EQUIV-WORLD-SECTION-001"],
  "notes": []
}
```

The fingerprint algorithm and normalized fingerprint are mandatory. The normalized fingerprint ignores comments/formatting while preserving literal values. `body_sha256` is diagnostic provenance for the exact pinned body and may change for non-semantic formatting edits without invalidating a normalized review.

`sync-records` rejects duplicate record IDs and multiple records claiming the same exact source method. It is idempotent: manual classifications and semantic/evidence edges are projected from the current record contents rather than appended forever.

A fingerprint or fingerprint-algorithm mismatch marks the source record `STALE`.

Review prose can remain in Markdown VAR documents; this JSON is the tracker sidecar that keeps status, provenance, staleness and evidence links queryable.
