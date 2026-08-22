# Representative-v1 seed-0 admission record

Status: **admitted member evidence; not a production decision**  
Parent: #19  
Target: Minecraft Java 26.2 / protocol 776 / data version 4903  
Representative policy: `vanilla-section-representative-v1`

This record preserves the first complete real member of the representative-v1 population. It is an admission/sanity record, not a benchmark winner declaration.

## Identity

- representative plan SHA-256: `fecb9c9bc77aa9689ceaf6d88fa9af96019a48d9533269f3bd15824f7dfc7191`;
- seed index: `0`;
- seed: `3250117973538344636`;
- official server SHA-256: `cdacdfb25898de5e4b4b0e5ddcc2722f77067e46605709c2d886c000ebb63ec5`;
- source inventory SHA-256: `e9ab0648c999c14c28d051145fbae2e31c82f4773e04aa989f9bc7929f87832e`;
- normalized corpus SHA-256: `01499b54b7347384b7c8b5a8a0f01d9a703816967b997564a6d5572c7c4ec29c`;
- GitHub Actions evidence artifact digest: `sha256:d357cc8037dfed336ec8c1d957e355daf9383e1f9c5840c34dac45010fd0c7f4`;
- admitted PR #41 head: `87bb2b54fdfb17ac24d29000a31e572aebcf7a98`;
- squash-merged policy commit: `c192a498d3a6a8cd5a8e0b37716f3db0bb80397d`.

## Admission chain

The exact PR head passed:

```text
frozen representative plan
        ↓
fresh official 26.2 runtime state probe
        ↓
committed source-qualification binding
        ↓
exact frozen generator-input digest
        ↓
official seed-0 world generation
        ↓
24 bounded 8-ticket batches
        ↓
exact selected-chunk extraction
        ↓
independent Python corpus validation
        ↓
all-five Rust reconstruction
        ↓
individual production-decision rejection
        ↓
evidence cross-check + artifact upload
```

Normal strict CI, benchmark smoke, full four-candidate semantic qualification, the parser corpus probe, and the representative-member probe were all green on the exact head.

## Population shape

The member contains:

- 3,584 sections;
- 14,680,064 cells;
- 432 distinct block-state IDs;
- 2,392 cardinality-1 sections.

Dimension lattices and section counts:

| Dimension | Section-Y lattice | Sections |
| --- | --- | ---: |
| Overworld | `-4..19` | 1,536 |
| Nether | `0..15` | 1,024 |
| End | `0..15` | 1,024 |

Semantic totals:

| Fact/class | Count |
| --- | ---: |
| non-air cells | 3,799,861 |
| counted-fluid cells | 236,812 |
| random-block cells | 129,840 |
| random-fluid cells | 112,992 |
| all-air sections | 2,365 |
| fluid-containing sections | 475 |
| random-block-present sections | 497 |
| random-fluid-present sections | 312 |

A useful invariant emerged immediately: the End contains **898 cardinality-1 sections but only 871 all-air sections**. Uniform representation and all-air semantics are therefore not interchangeable concepts. Future analysis must preserve that distinction.

## Deterministic owned-backing signal

These are object-plus-owned-backing totals from the existing deterministic accounting model, not RSS measurements:

| Candidate | Seed-0 owned bytes |
| --- | ---: |
| direct-reference | 29,417,472 |
| direct | 29,417,472 |
| adaptive | 2,987,328 |
| fast-local | 6,246,400 |
| packed-local | 2,186,664 |

On this one member, packed-local is about 26.8% below adaptive and about 92.6% below direct in deterministic owned backing; adaptive is about 89.8% below direct.

These differences are large enough to justify continuing the memory/CPU Pareto experiment. They are **not** evidence that packed-local is the production winner: this is one seed, logical owned bytes rather than process RSS, and no qualifying CPU/tail measurement has been performed.

### Per-dimension representation signal

Overworld:
- adaptive: 949 uniform, 495 local4, 92 local8;
- packed: 949 uniform, 5 packed-1, 12 packed-2, 182 packed-3, 296 packed-4, 83 packed-5, 9 packed-6.

Nether:
- adaptive: 545 uniform, 473 local4, 6 local8;
- packed: 545 uniform, 23 packed-1, 93 packed-2, 283 packed-3, 74 packed-4, 6 packed-5.

End:
- adaptive: 898 uniform, 125 local4, 1 local8;
- packed: 898 uniform, 111 packed-1, 6 packed-2, 2 packed-3, 6 packed-4, 1 packed-5.

This already shows why dimension-separated evidence matters: the representation distribution differs materially between generated Overworld, Nether and End terrain.

## Individual decision firewall

The member correctly fails the production decision gate with purpose:

```text
representative-member
```

and:

```text
decision_eligible = false
```

The complete four-seed population, not any individual member, is the minimum representative-v1 decision input.

## Post-admission audit findings

Auditing the four-member set path after this member passed found two additional evidence-integrity requirements before a complete population may be handed to performance qualification:

1. **generation-property identity** — set admission must compare every member's exact seed-specific `server.properties` contract, not merely server SHA/seed/plan identity;
2. **semantic-summary coherence** — per-dimension cell-fact and section-class summaries must use exact keysets, respect semantic bounds/subset relations, and recompose each member's global summaries exactly.

These findings did not invalidate the seed-0 corpus image. They harden how multiple members are admitted and labelled. PR #42 adds an independent population-admission firewall before any four-seed artifact can become benchmark-handoff eligible.

## Decision

- Admit seed 0 as real representative-member evidence.
- Preserve it as a sanity/data-distribution record.
- Do not use its candidate byte totals to select a production representation.
- Require the hardened complete four-seed set before target-hardware CPU/RSS qualification.
