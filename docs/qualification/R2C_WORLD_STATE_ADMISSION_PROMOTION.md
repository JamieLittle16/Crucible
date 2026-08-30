# R2C World-State Admission Promotion

Status: **promotion mechanism implemented; no biome/heightmap/light semantics are admitted by this document**  
Target: **Minecraft: Java Edition 26.2 / protocol 776 / DataVersion 4903**  
Parent: `R2C_WORLD_PROJECTION_QUALIFICATION.md`

## Purpose

This document defines the final source-free transition from the external R2C world-state admission bundle to committed repository evidence.

The relevant semantic groups are:

- `R2C-BIOMES`;
- `R2C-HEIGHTMAPS`;
- `R2C-LIGHT`.

The promotion step performs **no Minecraft semantic inference**. It cannot turn an unfinished review into admission, cannot regenerate human-authored SEM text, and cannot run without a successful independent Vanilla Atlas gate result.

## Evidence chain

The intended chain is:

```text
local official 26.2 source archive
        ↓
focused source review
        ↓
source-free completed review result
        ↓
human-authored semantic admission worksheet
        ↓
r2c_world_state_admission_materialize.py
        ↓
external source-free staging bundle
        ↓
vanilla_source_gate.py against pinned Atlas
        ↓
JSON report with admitted=true
        ↓
r2c_world_state_admission_promote.py
        ↓
canonical committed VAR / SEM / gate / report / admitted-bundle manifest
```

The source archive remains outside the repository throughout. Promotion consumes only source identities, fingerprints, reviewed hazards, authored semantic rules and the independent gate result.

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
- no extra, missing or symlinked staged file exists.

### Independent source gate

The report must prove:

- exact gate id;
- `admitted=true`;
- empty failure list;
- minimum status `VAR_REVIEWED`;
- exact staged `gate.json` SHA-256;
- Minecraft `26.2`, protocol `776`, DataVersion `4903`;
- exact pinned source-archive SHA-256 and fingerprint algorithm;
- exact `r2c-world-projection` frontier;
- exactly the staged VAR set is admitted;
- every admitted VAR's staged record SHA-256 matches;
- source identity and normalized/body fingerprints match the staged record;
- SEM linkage matches the staged record;
- reviewed hazards match and cover all currently observed Atlas hazards.

### Repository destination

- repository root must contain the expected `vanilla/` and `tools/` boundaries;
- no destination may already exist;
- no destination parent may be a symlink;
- all collisions are checked before any write begins.

The tool deliberately refuses overwrite/update semantics. A later change to admitted evidence must be a new reviewed/requalified repository change, not an in-place convenience rewrite hidden inside promotion.

## Operator sequence

After the review result and human-authored worksheet are complete:

```bash
cd ~/Helve

SRC="$HOME/Documents/mc-source/mc-src.zip"
DB=".helve/vanilla/atlas.sqlite"
STAGE="/tmp/helve-r2c-world-state-admission"
REPORT="/tmp/helve-r2c-world-state-source-gate.json"

rm -rf "$STAGE"
rm -f "$REPORT"

python3 tools/r2c_world_state_admission_materialize.py \
  --review-result /path/to/review-result.json \
  --worksheet /path/to/completed-admission-worksheet.json \
  --output-dir "$STAGE"

python3 tools/vanilla_atlas.py \
  --db "$DB" \
  verify-source "$SRC" \
  --lock vanilla/vanilla.lock.toml

python3 tools/vanilla_source_gate.py \
  --db "$DB" \
  --gate "$STAGE/gate.json" \
  --records "$STAGE/records" \
  --output "$REPORT"

python3 tools/r2c_world_state_admission_promote.py \
  --staging-dir "$STAGE" \
  --gate-report "$REPORT" \
  --repo-root .
```

The example source/Atlas paths are operator-local. They are not repository dependencies and must not be embedded in runtime code or committed evidence.

## Promotion manifest claim boundary

The final manifest intentionally distinguishes three claims:

```text
source_admitted = true
production_implementation_authorized = true
runtime_behavior_implemented = false
```

`source_admitted=true` means the reviewed SEM/VAR evidence passed the independent source gate.

`production_implementation_authorized=true` means Helve may now implement those admitted laws behind the semantic contract.

`runtime_behavior_implemented=false` prevents evidence promotion from pretending that biome, heightmap or light runtime behavior already exists or has passed differential/client qualification.

## Required follow-up after real promotion

Once real admitted artifacts are committed, R2C.4 still requires separate implementation and qualification:

1. implement Helve-native biome semantic state;
2. implement the admitted heightmap state/derivation contract;
3. implement the admitted light semantic state and lifecycle boundary;
4. compare imported state with independent official-save/oracle evidence;
5. preserve exact source/admission identities in the qualification record;
6. benchmark allocation, retained bytes and import/derived-state cost without weakening semantics;
7. only then allow the reference projector and later production projector to consume the new state.

## Non-claims

This promotion mechanism does **not** establish that:

- any biome, heightmap or light rule is currently admitted;
- the existing human-authored worksheet is complete;
- Helve has implemented those semantics;
- Mojang's internal representation should be copied;
- source admission alone is client-visible parity;
- hosted CI is performance admission.

It establishes only that, once the exact local source review and independent gate succeed, repository promotion is deterministic, source-free, byte-bound and fail-closed.
