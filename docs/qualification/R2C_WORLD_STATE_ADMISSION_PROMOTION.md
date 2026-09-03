# R2C World-State Admission Promotion

Status: **human source review complete; independent local Atlas admission and repository promotion pending**  
Target: **Minecraft: Java Edition 26.2 / protocol 776 / DataVersion 4903**  
Parent: `R2C_WORLD_PROJECTION_QUALIFICATION.md`

## Purpose

This document defines the final source-free transition from the reviewed R2C biome/heightmap/light decisions to committed repository evidence.

The relevant semantic groups are:

- `R2C-BIOMES`;
- `R2C-HEIGHTMAPS`;
- `R2C-LIGHT`.

The current source-free human decisions are committed under `vanilla/reviews/network/`. They still grant **no production admission** by themselves. The independent Vanilla Atlas gate remains mandatory before promotion or runtime reliance.

The promotion step performs **no Minecraft semantic inference**. It cannot turn an unfinished or stale review into admission, cannot regenerate human-authored SEM decisions, and cannot run without a successful independently bound gate result.

## Evidence chain

The current chain is:

```text
local official 26.2 source archive + pinned Atlas
        ↓
r2c_world_state_local_admission.py
        ├─ regenerate exact bounded source dossier in temporary storage
        ├─ bind committed human source-review decisions
        ├─ prepare + bind committed human SEM decisions
        ├─ materialize source-free VAR / SEM / gate staging
        └─ run independent Atlas gate against exact staging manifest
        ↓
source-free upload artifact + gate report
        ↓
explicit r2c_world_state_admission_promote.py
        ↓
canonical committed VAR / SEM / gate / report / admitted-bundle manifest
```

Official source excerpts exist only inside temporary local storage owned by the local runner. The published local admission artifact contains only source-free review/admission evidence and may be uploaded for inspection.

The R2C-specific source-gate wrapper is required because the generic Vanilla source gate directly binds the candidate gate and every VAR record, while the materialization manifest additionally content-addresses the staged SEM Markdown. `r2c_world_state_source_gate.py` hashes that exact manifest into the source-gate report, so the complete source-free staging bundle—including SEM text—is inside the admitted cryptographic envelope.

## Canonical promotion destinations

A successful promotion writes the already-admitted bytes to:

```text
vanilla/records/network/r2/world-state/<VAR>.json
vanilla/semantics/network/R2C_WORLD_STATE_SEMANTICS.md
vanilla/gates/network/GATE-NET-R2C-WORLD-STATE-26_2-001.json
vanilla/reports/r2c-world-state-source-admission-26.2.json
vanilla/reports/r2c-world-state-admitted-bundle-manifest.json
```

The VAR records, semantic Markdown, candidate gate and source-gate report are copied **byte-for-byte**. The promotion tool generates only the final admitted-bundle manifest.

## Fail-closed preflight

Before the first repository byte is written, promotion requires all of the following.

### Materialization identity

- schema 1;
- exact `r2c-world-state-admission-materialization` kind/id/policy;
- exact pinned source archive SHA-256;
- `contains_official_source_text=false`;
- `independent_gate_required=true`;
- `production_admitted=false` at the pre-gate stage;
- positive VAR and SEM counts;
- every manifest path, byte size and SHA-256 matches the staged file;
- VAR records live directly under `records/` and their filenames equal their VAR ids;
- no extra, missing or symlinked staged file exists.

### Independent source gate

The report must prove:

- exact gate id;
- `admitted=true`;
- empty failure list;
- minimum status `VAR_REVIEWED`;
- exact staged `gate.json` SHA-256;
- exact materialization id;
- exact materialization-manifest SHA-256;
- `source_free_bundle_bound=true`;
- Minecraft `26.2`, protocol `776`, DataVersion `4903`;
- exact pinned source-archive SHA-256 and fingerprint algorithm;
- exact `r2c-world-projection` frontier;
- exactly the staged VAR set is admitted;
- every admitted VAR's staged record SHA-256 matches;
- source identity and normalized/body fingerprints match the staged record;
- SEM linkage matches the staged record;
- reviewed hazards match and cover all currently observed Atlas hazards.

Because the manifest hashes the staged SEM Markdown as well as the VAR/gate files, requiring the exact manifest digest closes the whole source-free bundle rather than only the method records.

### Repository destination

- repository root must contain the expected `vanilla/` and `tools/` boundaries;
- no destination may already exist;
- no destination parent may be a symlink;
- all collisions are checked before any write begins.

The tool deliberately refuses overwrite/update semantics. A later change to admitted evidence must be a new reviewed/requalified repository change, not an in-place convenience rewrite hidden inside promotion.

## Canonical local admission command

The human source-review and semantic-decision records are already committed. On a machine that owns the pinned source archive and Atlas database, run:

```bash
cd ~/Helve
git switch main
git pull --ff-only

STAMP="$(date +%s)"
OUT="$HOME/Downloads/helve-r2c-world-state-admission-$STAMP.tar.gz"

python3 tools/r2c_world_state_local_admission.py \
  --db .crucible/vanilla/atlas.sqlite \
  --source "$HOME/Documents/mc-source/mc-src.zip" \
  --output "$OUT"

echo
echo "SOURCE-FREE ADMISSION ARTIFACT:"
echo "$OUT"
sha256sum "$OUT"
tar -tzf "$OUT"
```

The command regenerates and validates the exact current dossier rather than trusting an old local bundle. It fails closed if the source archive, plan/frontier, generated worksheet, committed review decisions, semantic decisions, materialized records or Atlas observations drift.

The resulting tar contains only source-free evidence:

```text
admission-run-manifest.json
gate-report.json
review/parent-review-result.json
review/prepared-admission-worksheet.json
review/completed-admission-worksheet.json
staging/manifest.json
staging/gate.json
staging/semantics/R2C_WORLD_STATE_SEMANTICS.md
staging/records/*.json
```

The local runner intentionally performs **no repository promotion**. A non-admitted Atlas result still produces this source-free artifact for diagnosis and exits nonzero.

## Explicit promotion after a green gate

Only after `gate-report.json` says `admitted=true`, unpack the exact source-free artifact and run promotion explicitly:

```bash
cd ~/Helve

EVIDENCE="/tmp/helve-r2c-world-state-admitted-evidence"
rm -rf "$EVIDENCE"
mkdir -p "$EVIDENCE"
tar -xzf /path/to/helve-r2c-world-state-admission-*.tar.gz -C "$EVIDENCE"

python3 tools/r2c_world_state_admission_promote.py \
  --staging-dir "$EVIDENCE/staging" \
  --gate-report "$EVIDENCE/gate-report.json" \
  --repo-root .
```

Promotion revalidates the complete staging/gate relationship independently; merely placing an `admitted=true` field in a report cannot bypass its digest, source, frontier, VAR, SEM or hazard checks.

`.crucible/vanilla/atlas.sqlite` is the repository's stable historical local-cache path and remains intentionally uncommitted despite the Helve rename. The source/Atlas paths are operator-local. They are not repository dependencies and must not be embedded in runtime code or committed evidence.

## Durable committed-evidence verification

Once the real admitted bundle is committed, `tools/r2c_world_state_admission_verify.py` re-hashes every promoted file against `r2c-world-state-admitted-bundle-manifest.json`. The dedicated `R2C World-State Admission Evidence` workflow runs this verifier automatically whenever the admitted evidence changes.

This source-free verifier detects repository drift. It does **not** replace a new Atlas qualification when the source archive, selected source methods, fingerprints, hazards or SEM contract changes.

## Promotion manifest claim boundary

The final manifest intentionally distinguishes three claims:

```text
source_admitted = true
production_implementation_authorized = true
runtime_behavior_implemented = false
```

`source_admitted=true` means the reviewed SEM/VAR evidence passed the independent source gate with the exact materialization manifest bound into the gate report.

`production_implementation_authorized=true` means Helve may now implement those admitted laws behind the semantic contract.

`runtime_behavior_implemented=false` prevents evidence promotion from pretending that biome, heightmap or light runtime behavior already exists or has passed differential/client qualification.

## Required follow-up after real promotion

Once real admitted artifacts are committed, R2C.4 still requires separate implementation and qualification:

1. retain the existing Helve-native 4×4×4 biome semantic substrate and bind target resolution only at projection;
2. derive the three admitted client heightmaps from Helve-authoritative block state rather than treating persisted Heightmaps NBT as live truth;
3. import sky/block nibble layers with an explicit light-correctness boundary; false/missing persisted `isLightOn` fails closed until relighting exists;
4. compose biome/heightmap/light state with resident chunk freshness without importing Mojang object topology;
5. compare imported/derived state with independent official-save/oracle evidence;
6. preserve exact source/admission identities in the qualification record;
7. benchmark allocation, retained bytes and import/derived-state cost without weakening semantics;
8. only then allow the reference projector and later production projector to consume the new state.

## Non-claims

The completed human review and this promotion mechanism do **not** establish that:

- the source gate has passed until a real local Atlas report says `admitted=true`;
- Helve has implemented the reviewed biome/heightmap/light semantics;
- Mojang's internal representation should be copied;
- source admission alone is client-visible parity;
- hosted CI is performance admission;
- world-entry, packet-id, pacing, block-entity or the remaining world-projection groups are admitted by this three-group gate.

They establish that the BIOMES/HEIGHTMAPS/LIGHT review is explicit and source-free, and that a successful local Atlas result can be promoted deterministically, byte-bound and fail-closed.