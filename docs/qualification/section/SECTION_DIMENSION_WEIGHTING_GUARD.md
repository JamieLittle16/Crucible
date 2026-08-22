# Section dimension-weighting firewall

Status: **normative M0.3D qualification rule**  
Parent: #19  
Applies to: `vanilla-section-representative-v1`

## Law

Representative vanilla section evidence is **dimension-separated evidence**.

No Crucible qualification tool may silently collapse Overworld, Nether and End candidate metrics into a production decision score unless a later profile explicitly declares and justifies a cross-dimension gameplay weighting.

The frozen representative plan already states:

```text
seed weighting      = equal
dimension weighting = report-separately
section weighting   = natural-within-selected-generated-chunks
```

The first implementation of the four-member set validator violated the middle rule even though the plan and documentation were correct. It summed candidate/cardinality evidence over all dimensions. Under the observed 26.2 lattices, each seed member contributes:

```text
Overworld : 64 × 24 = 1536 sections
Nether    : 64 × 16 = 1024 sections
End       : 64 × 16 = 1024 sections
```

A naïve total therefore imposes an accidental `1536:1024:1024 = 3:2:2` dimension weight. That ratio is a property of vanilla vertical section counts, **not** evidence that server workloads spend time in those dimensions in that ratio.

This defect was found before PR #41 merged and before any production representation was selected. No performance decision used the invalid aggregate.

## Required evidence shape

Every Rust representative-member import must retain, for each dimension independently:

- section count;
- total cells;
- distinct observed state count;
- cardinality histogram;
- all five candidate reconstruction identities;
- candidate owned-byte totals/maxima;
- candidate representation distributions;
- candidate construction transition counts;
- candidate logical backing-allocation counts.

The importer may additionally emit whole-member totals, but those totals are **consistency checks only**. Per-dimension evidence must recompose the whole-member histogram and candidate totals exactly.

The four-member set firewall then aggregates **across equal-weight seeds inside each dimension**, never across dimensions. Its decision-bearing structure is:

```text
per_dimension:
  minecraft:overworld: <four-seed aggregate>
  minecraft:the_nether: <four-seed aggregate>
  minecraft:the_end: <four-seed aggregate>
```

The set may expose a descriptive overall section/cell count, but that object must not contain candidate totals or a global cardinality histogram that could be mistaken for a decision metric.

The set record must state:

```text
decision_scope = dimension-separated-only
cross_dimension_score_allowed = false
```

## Seed independence

Representative-v1 requires four content-independent seeds. The four admitted member corpus SHA-256 values must be distinct.

Duplicate corpus identity is treated as a generation/admission failure, not as four independent samples. Given different frozen seeds and a broad 192-chunk selection, an identical normalized corpus would overwhelmingly indicate a reused/mislabeled world or another qualification defect.

## Regression obligations

Permanent tests must fail on at least:

- missing `per_dimension` Rust evidence;
- missing or extra dimension keys;
- per-dimension section/cell mismatch;
- per-dimension cardinality histogram that does not sum to its dimension;
- per-dimension histograms that do not recompose the member histogram;
- per-dimension candidate metrics that do not recompose the member candidate metrics;
- representation totals that do not equal dimension section counts;
- duplicate member corpus identities;
- any set artifact that permits an implicit cross-dimension decision score.

The official seed-0 representative-member probe independently exercises the same recomposition checks against a freshly generated pinned Minecraft 26.2 world.

## What remains allowed

Later profile decisions may deliberately combine dimensions, but they must make the assumption explicit, for example:

```text
profile = hypothetical-survival-a
cross_dimension_weights = {
  overworld = ...,
  nether = ...,
  end = ...,
}
```

Such weights are profile/workload evidence. They are not part of representative-v1 and cannot be inferred from section heights, save-file size, selected-chunk count, or corpus generation cost.

## General lesson

This is an instance of the broader Crucible rule:

> Preserve measurement strata until the model that justifies combining them is explicit.

A technically correct aggregation can still be a semantically invalid benchmark if it introduces an unstated workload model.
