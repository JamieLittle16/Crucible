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
    "normalized_sha256": "..."
  },
  "classifications": ["SEMANTIC_GAMEPLAY"],
  "hazards_reviewed": ["ORDERING"],
  "semantic_rules": ["SEM-WORLD-SECTION-001"],
  "evidence": ["EQUIV-WORLD-SECTION-001"],
  "notes": []
}
```

The fingerprint is mandatory once a record reaches semantic review. `sync-records` reports missing methods and marks changed fingerprints `STALE`.

Review prose can remain in Markdown VAR documents; this JSON is the tracker sidecar that keeps the evidence graph machine-readable.
