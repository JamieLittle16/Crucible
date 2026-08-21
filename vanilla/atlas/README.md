# Vanilla Atlas

The Vanilla Atlas is Crucible's machine-readable map from the pinned official Minecraft source corpus to semantic review and implementation evidence.

It exists to answer four questions continuously:

1. **What does the pinned vanilla source contain?**
2. **Which source surfaces may matter semantically?**
3. **What has Crucible reviewed/implemented/qualified?**
4. **What should we investigate next for the current milestone?**

The generated Atlas database is disposable local state. The official source archive is never committed. Human review records, semantic rules, evidence references, frontier definitions, and compact aggregate reports are version-controlled.

## Quick start

```bash
cargo xtask vanilla verify-source /path/to/mc-src.zip
cargo xtask vanilla index /path/to/mc-src.zip \
  --report-json vanilla/reports/26.2-source-audit.json \
  --report-md vanilla/reports/26.2-source-audit.md

cargo xtask vanilla status
cargo xtask vanilla coverage
cargo xtask vanilla frontier m0-world-kernel
cargo xtask vanilla next m0-world-kernel
cargo xtask vanilla show 'ServerChunkCache#getChunk'
cargo xtask vanilla deps 'ServerChunkCache#getChunk'
cargo xtask vanilla callers 'LevelChunkSection#setBlockState'
```

The default database is `.crucible/vanilla/atlas.sqlite`; override it with `--db` before the subcommand or `CRUCIBLE_ATLAS_DB`.

## Evidence model

```text
official source pin
      ↓
structural index + fingerprints        generated / disposable
      ↓
VAR review records                     version controlled
      ↓
SEM rules
      ↓
reference implementation
      ↓
parity evidence
      ↓
production implementation
      ↓
EQUIV + performance evidence
```

The v1 parser is conservative. It lexes the entire Java corpus, indexes types/methods/fields, tracks owner-field reads/writes, extracts syntactic call sites, conservatively resolves call targets when structural evidence is sufficient, and flags review hazards such as RNG, concurrency, ordering-sensitive collections, chunk access, scheduled ticks, neighbor updates, protocol surfaces, persistence and codecs.

A hazard is **not** a semantic conclusion. It is a prioritised review lead. The tool deliberately never auto-classifies a method as `IMPLEMENTATION_ONLY`.

## Fingerprint law

Review records are pinned to `java-token-v2-literal-sensitive` fingerprints. The canonical fingerprint ignores comments and formatting but **preserves numeric, string and character literal values**. This is intentional: a change such as a random-tick divisor, protocol constant, timeout, dimension, threshold or registry literal can be semantic and must invalidate old review evidence.

The Atlas also stores a raw method-body digest for diagnostics. Raw body changes do not by themselves make a record stale when the literal-sensitive normalized token fingerprint is unchanged.

## Review lifecycle

Machine-readable source records use one of:

```text
UNSEEN
INDEXED
CLASSIFIED
VAR_REVIEWED
SEM_EXTRACTED
REFERENCE_IMPLEMENTED
REFERENCE_PARITY
PRODUCTION_IMPLEMENTED
EQUIVALENCE_QUALIFIED
PERFORMANCE_QUALIFIED
INTEGRATED
STALE
```

Create a fingerprint-pinned record skeleton with:

```bash
cargo xtask vanilla record-template \
  'LevelChunkSection#setBlockState(...)' \
  --id VAR-WORLD-SECTION-001
```

Store reviewed records below `vanilla/records/`, then:

```bash
cargo xtask vanilla sync-records
cargo xtask vanilla stale
```

`sync-records` is idempotent: the version-controlled record is authoritative for its manual classifications and evidence edges. Removed record fields are removed from the generated projection on the next sync rather than accumulating stale database rows.

A fingerprint or fingerprint-algorithm mismatch turns a record `STALE` rather than silently accepting an old conclusion. Stale methods are deliberately promoted by `vanilla next` because invalidated evidence is urgent work, not advanced progress.

## Frontiers

`vanilla/frontiers/*.json` define implementation/review frontiers. A frontier is a set of source roots plus a bounded dependency closure. It is deliberately not a claim that every reachable Java method must be reproduced.

`vanilla next <frontier>` ranks unqualified methods using consequence signals such as hazard classes, graph connectivity, owner-field mutation, unresolved edges, and whether the method is an explicit frontier root.

This gives Crucible a source-backed work queue instead of a hand-wavy notion of subsystem completeness.
