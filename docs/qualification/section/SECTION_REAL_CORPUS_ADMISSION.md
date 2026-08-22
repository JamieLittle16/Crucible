# Official 26.2 Section Corpus Admission Record

Status: **PASS — extractor admitted against a real pinned Minecraft Java 26.2 world**  
Parent: #19  
Admission PR: #37  
Corpus policy: `vanilla-save-region-v1-stored-sections`

This record freezes the first end-to-end real-target admission of Crucible's vanilla-save section extractor. It is an evidence record for **save-format/parser correctness**, not a production workload-weighting record.

## Evidence chain

```text
pinned official Minecraft 26.2 server
        ↓
fresh official runtime state reflection
        ↓
committed source-qualification binding
        ↓
exact frozen qualified-state input digest
        ↓
deterministic official spawn-world generation
        ↓
vanilla save-region/NBT extractor
        ↓
CRUCIBLE-SECTION-CORPUS/1
        ↓
independent corpus validator
```

Every stage passed on the same GitHub Actions run.

## Target identities

- Minecraft: `26.2`
- protocol: `776`
- data version: `4903`
- state count: `32,366`
- official server SHA-256: `cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5`
- frozen qualified-state input SHA-256: `2cbdfa96881be3f47dee8afdb5c4006e42cf2a718c440bf8ae76761a6acd31af`
- source+runtime qualified-state digest: `8f12bb4a3636aa258929eeb963c7d9512256bfe37d01d9a9c5821e7947b1aed6`
- generated state-data digest: `79e5803347d6fb6f7ffccea4cef783998a1c6469ed869d26fa48ab5f2328cd3b`

The fresh official-runtime dataset bound through the committed source qualification reproduced the exact frozen qualified-state generator input before save extraction was allowed to proceed.

## Deterministic official world

Generator identity: `official-server-spawn-world-v1`

- seed: `6842363988700132471`
- view distance: `2`
- simulation distance: `2`
- difficulty: peaceful
- gamemode: survival
- max players: `1`
- Nether disabled for this parser-admission world
- synchronous chunk writes enabled
- loopback-only server binding
- explicit `save-all flush` followed by clean `stop`

The real 26.2 Overworld region path observed and required by the admitted tooling is:

```text
dimensions/minecraft/overworld/region
```

## Real-target defect found during admission

The first official-world probe failed before extraction because the extractor still assumed the historical default dimension layout:

```text
region/
DIM-1/region/
DIM1/region/
```

Minecraft 26.1 changed default dimension storage to the namespaced hierarchy used by the pinned 26.2 target. The extractor, synthetic fixtures, world-probe postcondition and documentation were corrected to the target layout:

```text
dimensions/minecraft/overworld/region
dimensions/minecraft/the_nether/region
dimensions/minecraft/the_end/region
```

A permanent regression now rejects legacy root `region/` as a standard 26.2 dimension location. The same hardening pass added rejection of overlapping Anvil sector allocations.

This defect is retained because it demonstrates an important Crucible rule: historical Minecraft conventions are not target evidence. Tooling-visible filesystem semantics are version-pinned just like registry IDs and protocol data.

## Admitted real corpus

Source inventory SHA-256:

`54279c23aa14032f1000ec82aad930b3b3f18af5c36210751b64db0aa321960e`

Normalized corpus SHA-256:

`8f1b623f4cd323ff8072c3c2722f96190dfe49b624ae65cf612f1e5ba785febf`

Workflow artifact ZIP SHA-256:

`e4d13661b538df496ac4758182112ef1c68b7f4c4cd19d20001231e01cee2361`

Artifact identity:

- workflow: `Section Corpus Probe`
- run: `32578460546`
- artifact ID: `9477198544`
- artifact name: `minecraft-26.2-section-corpus-probe`

### Source snapshot

The source inventory contained:

- `level.dat`;
- four Overworld `.mca` region files under the 26.2 namespaced dimension path.

The inventory is content-addressed; transient filesystem timestamps are not part of corpus identity.

### Corpus totals

- stored sections: **12,696**
- normalized block cells: **52,002,816**
- distinct observed state IDs: **81**
- dimensions: Overworld only

Aggregate semantic cells:

- non-air: `759,382`
- counted fluid: `8,959`
- random-block: `4,626`
- random-fluid: `4,019`

Section classes:

- all-air: `12,452`
- contains fluid: `46`
- random-block present: `52`
- random-fluid present: `15`

### Cardinality histogram

| Cardinality | Sections |
|---:|---:|
| 1 | 12,453 |
| 2 | 34 |
| 3 | 33 |
| 4 | 26 |
| 5 | 18 |
| 6 | 19 |
| 7 | 12 |
| 8 | 14 |
| 9 | 19 |
| 10 | 17 |
| 11 | 11 |
| 12 | 6 |
| 13 | 5 |
| 14 | 11 |
| 15 | 2 |
| 16 | 4 |
| 17 | 2 |
| 18 | 4 |
| 19 | 2 |
| 20 | 2 |
| 21 | 2 |

Only **243** stored sections have cardinality greater than one.

## Interpretation

This corpus proves that the admitted extractor can consume a real official 26.2 save and normalize its actual section state containers through the frozen semantic state identity system.

It does **not** prove that this tiny spawn world represents production Minecraft workloads. In fact, its distribution shows why that distinction matters: 12,452 of 12,696 stored sections are all-air and 12,453 have cardinality one. Using this corpus directly as the final performance weighting would heavily reward Uniform representation behavior and under-sample active terrain/build/churn sections.

Therefore this corpus is classified as:

```text
purpose = parser-admission
weighting = non-qualifying
```

It may be used for parser regression, corpus-ingestion tests, exploratory real-layout benchmarks and reproducibility checks. It must not independently select a production representation or adaptive threshold.

## Permanent regression obligations

The real-corpus pipeline must continue to reject or detect:

- target Minecraft/protocol/data-version drift;
- official server hash drift;
- source/runtime state-identity drift;
- legacy pre-26.1 standard-dimension paths for target 26.2;
- malformed/overlapping Anvil sector allocations;
- region-slot/chunk-coordinate disagreement;
- wrong NBT container types;
- wrong packed-long counts or out-of-range palette indices;
- unknown canonical block-state identities;
- corpus/state-data digest mismatch;
- corpus/inventory digest disagreement.

## Next evidence layer

The next M0.3D slice consumes `CRUCIBLE-SECTION-CORPUS/1` in the Rust benchmark laboratory while preserving corpus identity and purpose classification.

After that, final production selection still requires a broader representative corpus policy that:

1. accounts correctly for semantically present omitted/all-air sections;
2. samples active terrain, exploration, builds, fluid-bearing and high-entropy regions;
3. covers relevant standard dimensions;
4. records sampling/weighting policy explicitly;
5. is qualified against controlled target hardware with process/RSS and noise analysis.

No representation winner is selected by this admission record.
