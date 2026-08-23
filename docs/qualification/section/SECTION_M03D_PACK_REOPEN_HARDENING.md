# M0.3D benchmark-pack reopen hardening

Parent: #19  
Supplements: [`SECTION_M03D_FINAL_QUALIFICATION.md`](SECTION_M03D_FINAL_QUALIFICATION.md)  
Status: **final-session evidence-integrity hardening; no production-policy decision**

## Purpose

The final M0.3D driver already content-addressed the benchmark-pack manifest at creation and recursively hashed the generated pack tree in the final session artifact manifest.

This hardening closes the remaining temporal gap between those two boundaries:

> The exact benchmark-pack manifest admitted at pack creation must still be the manifest present immediately before the qualification session is sealed.

A generated evidence file appearing in the recursive final inventory is not sufficient by itself. The top-level session must fail closed if the benchmark-pack identity that the measurement was built from changes during the experiment.

## Reopen rule

`tools/section_m03d_qualification.py` now reopens `packs/pack-manifest.json`:

1. immediately after the pack builder returns; and
2. again after measurement/Pareto work and the external representative/correctness inputs have been revalidated, before any top-level session seal is written.

At each reopen the driver requires:

- `pack-manifest.json` is a real non-symlink file;
- the JSON root is an object;
- `decision_scope = dimension-separated-only`;
- `cross_dimension_score_allowed = false`;
- the manifest's canonical digest recomputes exactly;
- the recomputed `manifest_sha256` equals the digest frozen at pack creation;
- representative policy identity is unchanged;
- representative population SHA-256 is unchanged;
- population-admission SHA-256 is unchanged;
- source representative-artifact manifest SHA-256 is unchanged.

Therefore rewriting the pack manifest and recomputing its own internal digest does not make the mutation acceptable: the original in-session identity remains frozen independently.

## Permanent regression

`tools/tests/test_section_m03d_pack_revalidation.py` simulates a completed but noise-ineligible measurement layer that mutates the pack manifest after pack creation, adds new content, and recomputes the manifest's own canonical digest.

The driver must reject the session before `qualification-session.json` or `session-artifact-manifest.json` is written.

This test deliberately uses an ineligible measurement outcome so Pareto analysis is not part of the failure. It isolates the invariant being tested: **mid-session benchmark-pack identity drift alone is sufficient to invalidate the final seal**.

## Decision boundary

This change does not alter:

- the representative-v1 workload policy;
- section candidate implementations;
- measurement scheduling;
- noise thresholds;
- dimension separation;
- Pareto rules;
- production-policy selection.

It only narrows which evidence histories may produce a completed M0.3D qualification session.
