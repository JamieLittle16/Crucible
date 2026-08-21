# Vanilla Atlas SQLite Schema v1

The SQLite database is generated under `.crucible/` and must not be committed. It contains structural metadata and hashes, not Mojang source bodies.

## Identity

`meta` records Atlas/schema versions, the fingerprint algorithm, Minecraft/protocol/world versions and the exact official source archive SHA-256.

`source_files` records path, package, file digest and line/byte counts.

`types`, `methods`, and `fields` provide stable structural inventory. Method identity combines the owning qualified type, source signature and normalized token fingerprint. Line numbers are navigation hints, not identity.

The v1 fingerprint algorithm is `java-token-v2-literal-sensitive`: comments and formatting are discarded, while identifiers, operators and literal values are retained. This allows harmless layout edits to remain stable without masking semantically relevant constant changes.

`methods.body_sha256` records the exact raw source body digest as secondary provenance. Human review records use the normalized fingerprint for staleness.

## Dependency data

`method_calls` stores every syntactic call site. `resolution='syntactic'` means no target has been proven. `resolved_method_id` is populated only by conservative resolution rules such as same-type dispatch, explicit imported type use, or a uniquely identifiable owner field type.

`field_accesses` records accesses to fields declared by the owning type with modes `read`, `write`, and `read_write`.

## Review leads

`hazards` stores mechanically observed review leads. Hazard presence does not imply semantic significance.

`classifications` stores both heuristic and manual labels. The `source` column distinguishes them; heuristic labels must never silently become manual conclusions.

## Human evidence projection

`tracking` is the generated projection of version-controlled records under `vanilla/records/`.

`semantic_edges` links a source method/VAR to SEM and evidence identifiers. `sync-records` rebuilds each record's manual projection idempotently so removed classifications/edges do not linger.

Later Atlas schemas may normalize SEM/EQUIV/benchmark artifacts into dedicated tables, but v1 intentionally keeps the source tracker small.

## Upgrade law

Database schema changes require an explicit `SCHEMA_VERSION` increment. Fingerprint-algorithm changes require a new named algorithm and will intentionally stale records pinned to the previous algorithm. The database is rebuilt from source rather than migrated in place unless a future use case demonstrates that migration has value.
