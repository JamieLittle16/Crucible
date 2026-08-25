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

## Static declaration evidence

Java class-initialization law can live outside ordinary source methods, especially protocol packet registrations, `StreamCodec` declarations and enum constants carrying protocol/state IDs. After `tools/vanilla_declaration_index.py` augments a generated Atlas, such source-level class initialization is exposed under the reserved synthetic signature `<clinit>()`.

A `<clinit>()` row is **not a claim that Mojang wrote a method with that name**. It is an evidence-only projection of that type's enum-constant preamble, initialized static fields and static initializer blocks, concatenated in source order. Its normalized fingerprint uses the same literal-sensitive Java-token algorithm; its exact body fingerprint hashes the corresponding raw source spans with unambiguous separators. Methods, constructors, instance fields and instance initializer blocks are excluded. Nested types receive their own independent `<clinit>()` node.

This projection lets the existing VAR/source-gate machinery pin declarations that materially define protocol semantics without copying Mojang source into Git or laundering a declaration fact through an unrelated method.

## Instance field initializer evidence

Observable source law can also live in initialized instance fields, particularly anonymous callback implementations. `tools/vanilla_instance_field_index.py` projects each simple initialized instance-field declaration into one synthetic Atlas node with signature:

```text
<fieldinit:FIELD_NAME>()
```

A `<fieldinit:...>()` row is likewise **evidence tooling, not a Java method claim**. Its fingerprint covers the complete declaration from the field prefix through the terminating semicolon, including anonymous-class, lambda, array and object-initializer bodies. Static fields and interface fields are excluded because their initialization belongs to `<clinit>()`; uninitialized fields, method-local variables and multi-declarator declarations are excluded rather than guessed.

This node exists specifically so source law hidden in an anonymous instance-field implementation can use the ordinary VAR/source-gate staleness machinery. The synthetic evidence node owns no runtime abstraction and implies no Crucible implementation shape.

`sync-records` rejects duplicate record IDs and multiple records claiming the same exact source method/evidence node. It is idempotent: manual classifications and semantic/evidence edges are projected from the current record contents rather than appended forever.

A fingerprint or fingerprint-algorithm mismatch marks the source record `STALE`.

Review prose can remain in Markdown VAR documents; this JSON is the tracker sidecar that keeps status, provenance, staleness and evidence links queryable.
