# Section Representative Corpus Policy

Status: **M0.3D representative-v1 implementation**  
Parent: #19  
Target: Minecraft Java 26.2 / protocol 776 / data version 4903

This document freezes the first decision-oriented real-vanilla sampling policy for the section representation laboratory.

It builds on `docs/qualification/SECTION_VANILLA_CORPUS.md`. The canonical `CRUCIBLE-SECTION-CORPUS/1` member format does not change.

## Why a second corpus policy exists

The admitted spawn-world corpus proves the save/extractor/import path. It is intentionally not a production weighting corpus: one spawn world is overwhelmingly homogeneous/all-air and reflects one seed plus Mojang's spawn-generation footprint.

M0.3D needs a real vanilla-derived population large and broad enough to reveal whether synthetic representation boundaries matter in actual generated terrain. The selection itself must be independent of observed benchmark results.

The representative-v1 policy therefore freezes **how samples are chosen before their content is inspected**.

## Policy identity

```text
policy = vanilla-section-representative-v1
plan schema = 1
plan_sha256 = fecb9c9bc77aa9689ceaf6d88fa9af96019a48d9533269f3bd15824f7dfc7191
```

The plan is generated and verified by:

```bash
python3 tools/section_representative_plan.py write /tmp/section-plan.json
python3 tools/section_representative_plan.py verify /tmp/section-plan.json
```

The plan generator itself is covered by frozen seed/digest tests. A change to any seed, coordinate, weighting or selection guard creates a different plan digest and therefore a different evidence population.

## Dimension abstraction

Representative qualification follows the same architectural direction as the engine: **dimension identity/configuration is data; mechanisms operate over descriptors rather than branching on dimension names**.

Cold qualification tooling has two deliberately small descriptor layers:

```text
VanillaDimensionDescriptor
    key                 e.g. minecraft:overworld
    26.2 region path    e.g. dimensions/minecraft/overworld/region
              │
              ▼
RepresentativeDimensionDescriptor
    vanilla descriptor reference
    anchor coordinates
    sampling radius
    coordinate-policy identity
    optional minimum Chebyshev radius
```

`tools/vanilla_dimensions.py` is the qualification-side source of truth for standard target-visible dimension identity and the pinned 26.2 save topology. `tools/section_representative_plan.py` adds only representative-sampling policy.

The world generator, representative extractor and expected-region checks iterate those descriptors generically. The End's outer-island sampling rule is descriptor data (`minimum_chebyshev_radius = 80`), not an `if dimension == minecraft:the_end` branch in the mechanism.

This module is **not** the future production Crucible `Dimension` runtime object. It intentionally does not contain mutation authority, scheduler ownership, ticking state, component/profile resolution or gameplay services. Production code must not import the qualification descriptor. The two systems share an architectural law, not an implementation dependency:

> Dimension identity and configuration are explicit data; subsystems consume the properties they need.

Representative-v1 deliberately supports only the three pinned standard vanilla dimensions. Unknown/custom dimensions fail closed. Supporting additional dimension classes later requires explicit evidence and, if it changes the benchmark population, a new representative policy rather than a hidden branch.

## Seed selection

Representative-v1 uses exactly four vanilla seeds.

They are not hand-selected. They are the first four signed big-endian 64-bit words of:

```text
SHA-512("Crucible|Minecraft-Java-26.2|section-representative-v1")
```

which yields:

```text
0:  3250117973538344636
1: -1983012757746938611
2:  4735876718611431443
3:  3964809773196812219
```

This is intentionally content-independent. A seed may not be removed because its terrain looks inconvenient or because it changes which representation appears best.

## Chunk sampling

Each seed uses exactly **64 selected chunk columns in each standard dimension**:

- `minecraft:overworld`;
- `minecraft:the_nether`;
- `minecraft:the_end`.

The same chunk-coordinate schedule is used for every seed. This means seed variation is measured independently of coordinate-schedule variation.

Total selected chunk columns:

```text
4 seeds × 3 dimensions × 64 chunks = 768 chunk columns
```

### Overworld and Nether

Both use a small fixed anchor set plus SHA-256-derived unique coordinates in a square of radius 2048 chunks.

The anchors guarantee local/origin and moderate-distance material. The hashed remainder supplies broad spatial/biome variation without inspecting world content.

### End

The End uses central-island anchors plus SHA-256-derived samples in a square of radius 512 chunks. Hashed points inside the central ±79-chunk square are rejected, so nearly all non-anchor samples exercise outer-End terrain rather than repeatedly measuring the central island/void transition.

The coordinate schedule is generated by `tools/section_representative_plan.py`; the documentation intentionally does not duplicate all 192 coordinates as a second source of truth.

## Official-world generation

One member world is generated with the pinned official 26.2 server by:

```bash
python3 tools/official_representative_section_world.py \
  --version 26.2 \
  --plan /tmp/section-plan.json \
  --seed-index 0 \
  --work-dir .crucible/vanilla/representative/seed-0 \
  --batch-size 8 \
  --batch-settle-seconds 2 \
  --evidence .crucible/vanilla/representative/seed-0/world-evidence.json
```

### Frozen selection, replaceable orchestration

The representative plan defines **which** 192 dimension/chunk tickets belong to one seed member. It does not freeze the server-orchestration mechanism used to materialize those chunks.

The stable selection-command identity remains:

```text
192 `forceload add` commands
selection_command_sha256 = cb97b7490c28e38293251561749a87dbda2d0f78d78c7cf98471e5eff825a354
```

The current admitted generator mechanism is:

```text
official-server-representative-section-world-v2-batched
```

It converts dimension-scoped chunk tickets into bounded, same-dimension batches. For every batch:

```text
install <= batch_size force-load tickets
        ↓
unique console marker proves commands were processed
        ↓
bounded generation settle window
        ↓
save-all flush
        ↓
unique console marker proves the synchronous flush returned
        ↓
remove exactly that batch's force-load tickets
        ↓
unique console marker proves removals were processed
        ↓
next batch
```

After all batches, a final `save-all flush` barrier runs before shutdown.

The generator therefore keeps resource pressure bounded instead of holding the complete widely separated population force-loaded simultaneously. This mirrors Crucible's broader resource-governance doctrine: batch boundaries and explicit completion barriers are preferred to unbounded admission plus hope.

### Why v1 generation was rejected

The first real representative-member probe on 2026-08-22 used generator v1:

```text
192 live force-load tickets
→ fixed 90-second settle
→ one save-all flush
→ stop
```

The frozen plan, official runtime state binding and all policy tests passed. The official server did not crash, but generation/save backlog remained large enough that it could not stop cleanly before the 600-second generator deadline.

This was classified as an orchestration/resource-governance failure, **not** a corpus-selection failure. The sample was not shrunk or cherry-picked. Generator v1 is superseded by bounded v2 and remains part of the experiment record.

### Generator evidence

World evidence schema 2 records:

- exact generator identity;
- pinned official server SHA-256;
- representative policy and plan digest;
- seed index and seed;
- stable 192-command selection digest;
- batch size/count;
- settle policy;
- one timing/coverage record per batch;
- exact server properties.

Batch elapsed times are validated as diagnostics but are deliberately excluded from the stable corpus-set identity. Runner speed is not workload identity.

The world may contain extra spawn/support chunks generated by Mojang. Those chunks are allowed in the source world but are **not** admitted into the representative corpus.

## Member extraction

`tools/representative_section_corpus.py` consumes one generated world and emits one canonical corpus member.

The extractor is identified as:

```text
vanilla-save-region-v2-representative-member
```

For every dimension descriptor it filters to exactly the 64 plan coordinates. It requires:

- every selected chunk to exist;
- no unplanned chunk to enter the output;
- every selected chunk to expose a non-empty section-Y lattice;
- each chunk's lattice to be contiguous;
- all selected chunks in one dimension to have the exact same lattice;
- the final section count to equal `64 × lattice-height` for each dimension.

This is deliberately stricter than filling guessed air sections. If the pinned target's save semantics ever stop exposing a complete contiguous block-section image, representative-v1 fails closed and must be revised with new vanilla evidence.

The admitted 26.2 spawn corpus already provides a useful sanity signal: `12,696 = 529 × 24`, showing a complete 24-section Overworld image for every generated chunk in that probe.

## Why member corpora are not individually decision-eligible

A single representative member is known to the Rust importer as:

```text
purpose = representative-member
decision_eligible = false
```

That is intentional. One seed cannot stand in for the four-seed population merely because it was generated with the representative extractor.

`section_bench --corpus-decision-check` must therefore continue to reject each individual member.

## Corpus-set decision gate

`tools/section_corpus_set.py` is the population-level decision firewall.

It requires exactly four members and independently cross-checks, for each seed:

- bounded official representative-world generator identity;
- canonical pinned server SHA-256;
- stable selection-command count/digest;
- valid bounded-batch structure and complete per-dimension ticket coverage;
- representative policy and plan SHA-256;
- seed index and exact derived seed;
- exact selected chunk schedule;
- canonical source inventory SHA-256;
- canonical corpus SHA-256;
- Python manifest target/source/counts;
- Rust target/source/purpose/counts;
- Python↔Rust cardinality and distinct-state agreement;
- all five current section mechanisms and their representation totals;
- non-eligibility of the individual member;
- per-dimension contiguous section lattice.

It then requires the dimension lattice to agree across all four seeds.

Only after all of those checks does the set record contain:

```text
decision_eligible = true
```

The set is itself content-addressed by a canonical `set_sha256`.

## Weighting

Representative-v1 freezes these rules:

```text
seed weighting      = equal
dimension weighting = report-separately
section weighting   = natural within selected generated chunks
```

### Why dimensions are not collapsed

We do not currently possess a defensible universal statement such as "a server spends 82% of section accesses in the Overworld". Such a number would vary dramatically by server type.

Therefore representative-v1 does **not** invent a cross-dimension gameplay weighting. Performance and memory evidence is reported separately for Overworld, Nether and End.

A candidate that is only attractive under an assumed dimension mix must state that assumption explicitly in a later profile/decision artifact.

### Why vertical sections are natural-weighted

Every section in each selected generated chunk contributes once. This preserves the vanilla-generated vertical storage distribution, including homogeneous air/solid sections. We do not rebalance away homogeneous sections simply because they strongly reward uniform storage; doing so would distort actual section populations.

## What this corpus represents—and what it does not

Representative-v1 is evidence for:

- naturally generated vanilla section cardinalities;
- naturally generated spatial block-state images;
- relative prevalence of uniform/local/high-cardinality terrain sections;
- dimension-specific representation/memory distributions;
- whether synthetic thresholds correspond to material real-world populations.

It is **not** evidence for:

- real player dimension-time weighting;
- build-server block distributions;
- redstone-heavy mutation rates;
- mining/destruction traces;
- combat/tick workload mix;
- live cache residency across a full server;
- allocator/process RSS by itself.

Those questions remain covered by controlled synthetic traces, future whole-server workloads, and target-hardware qualification.

## Sample size

The expected section count is determined by the real 26.2 section lattice rather than hardcoded height constants.

If the observed standard lattices remain 24 Overworld sections and 16 Nether/End sections, representative-v1 contains approximately:

```text
4 × 64 × (24 + 16 + 16) = 14,336 section images
```

or roughly 58.7 million section cells. The exact count is evidence and must be derived from the generated worlds.

## CI versus qualification hardware

Hosted CI may generate **one complete seed member** to prove the official-server commands, bounded batching/barriers, multi-dimension save path, selection/lattice rules, Python validator and Rust importer remain target-correct.

Hosted CI timing is never used for production selection. Batch timings in the member artifact diagnose generation orchestration only.

If a complete member becomes too expensive for routine PR execution, the sampling policy is not weakened. The full member moves to a manual/scheduled qualification workflow and a smaller target-format smoke remains on PRs.

The complete four-seed set is generated as a qualification artifact and later consumed by the controlled target-hardware benchmark/RSS protocol under #19.

## Module boundaries

The representative path is intentionally split so failures localize cleanly:

```text
vanilla_dimensions.py
    target-visible dimension/save descriptors

section_representative_plan.py
    content-independent seed/chunk sampling law

official_representative_section_world.py
    bounded official-server materialization mechanism

representative_section_corpus.py
    exact selected-chunk extraction + lattice proof

section_corpus.py
    independent Python semantic validation

section_bench/corpus/*
    independent Rust streaming reconstruction

section_corpus_set.py
    four-member decision firewall / aggregate identity
```

No production section implementation is modified by this qualification path.

## Testing obligations

Permanent tests cover at least:

- exact frozen seed derivation and plan SHA;
- descriptor key/path identity and uniqueness;
- descriptor-driven sampling output remaining byte-identical to the frozen plan;
- coordinate uniqueness/quadrant/End-outer coverage;
- dimension-local bounded batches;
- exact ticket add/remove inversion;
- complete 192-ticket batch coverage with no duplicates;
- orchestration phase ordering with a fake console;
- invalid batch sizes;
- negative region-coordinate floor division;
- missing selected regions/chunks;
- unplanned chunk exclusion;
- duplicate/gapped/inconsistent section lattices;
- exact member purpose and individual decision rejection;
- canonical SHA-256 formatting;
- bounded-generator provenance corruption;
- missing/altered four-member populations;
- cross-seed lattice drift;
- Python↔Rust histogram/count disagreement;
- candidate-set/representation-total corruption.

A real pinned-server member probe sits above these synthetic/adversarial tests.

## Production decision chain

Representative-v1 does not select a winner by itself.

The M0.3D decision remains:

```text
synthetic boundary/tail benchmarks
            +
representative-v1 vanilla corpus set
            +
per-dimension real-corpus measurements
            +
controlled target-hardware process/RSS evidence
            +
noise/repetition analysis
            ↓
Pareto + complexity decision record
            ↓
selected production policy / documented losers
```

The direct reference remains a permanent correctness oracle and never becomes a production winner.

## Change control

Any change to:

- seed count or seed derivation;
- chunk count;
- anchor coordinates;
- hash-coordinate algorithm/range;
- End inner-region rule;
- dimensions included;
- seed weighting;
- dimension weighting;
- section weighting;
- selected-chunk completeness rules;
- lattice rules;

creates a new representative policy and plan digest.

Changes to bounded generation mechanics do **not** silently change the representative sampling plan. They must change the generator identity/evidence and pass the same exact corpus admission chain.

Do not silently regenerate `representative-v1` with altered sampling rules.
