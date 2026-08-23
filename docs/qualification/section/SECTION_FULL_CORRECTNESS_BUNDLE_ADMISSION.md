# Full Section Correctness Bundle Admission — 2026-08-22

Supports: #18 and #19  
Implementation checkpoint: `52eb3ba30b6d3880eb476583f53a4961ff665f5e`  
Status at checkpoint: **ordinary strict CI PASS + Section Full Qualification PASS**

## Purpose

The full M0.3C correctness surface is available as one independently validated same-checkout handoff artifact for M0.3D decision analysis.

The existing four-way matrix remains the primary parallel semantic gate. Only after all four matrix jobs pass, a second sealing job reruns `direct`, `adaptive`, `fast-local`, and `packed-local` sequentially in one exact checkout and packages the resulting full records together.

This avoids relying on cross-job artifact association when the final target-hardware/Pareto evidence chain needs one unambiguous correctness set.

## Admission evidence

On the implementation checkpoint:

- ordinary strict CI: PASS;
- full qualification matrix, all four candidates: PASS;
- same-checkout regeneration, all four candidates: PASS;
- correctness-bundle validator: PASS;
- validated bundle upload: PASS.

GitHub artifact:

- name: `crucible-section-full-bundle`;
- artifact SHA-256: `782db5e18e82035478b9b57c377e93e5332dab413e44a775d154a1f3ef04c2ea`;
- bundle-manifest SHA-256 identity: `2a4051ee46df9a03ca5781992888e6571b0fc3fd852d4ed63c2507b6453ba479`.

The inspected manifest contains exactly:

- `direct/full.json`;
- `adaptive/full.json`;
- `fast-local/full.json`;
- `packed-local/full.json`;
- `bundle-manifest.json`.

Candidate evidence-file SHA-256 values at this PR checkpoint:

- direct: `4044f120693812881d69cd39296a5e48c940608142b4bf233f10fe29307168dd`;
- adaptive: `fdc988a43d4c47a531b4625a3563528cbc037c73abc0c868d93816cf8d87b0ac`;
- fast-local: `b11239e1d99cd623b810b4603abe0a74d4111bc8e447ec19a77bd96cdcda3b79`;
- packed-local: `a388eb073d8ff712e92dc33174276cc8a780f7af9aeb15ae12f173191d1334f6`.

All four records carry:

- Minecraft 26.2;
- protocol 776;
- data version 4903;
- 32,366 block states;
- target generation/input digests frozen by M0.3A;
- trace schema 1;
- 16 deterministic traces;
- 2,013,879 trace operations;
- 4,112 synthetic operations;
- FNV checkpoint `6a4814a1551a9e5a`;
- the exact current M0.3C SEM surface.

## Pull-request commit identity note

The bundle generated on a `pull_request` workflow is bound to GitHub's synthetic PR merge checkout commit (`74047d731fa56a97b8998817aa800e04b82011de` at this checkpoint), not to the branch-head SHA. This is expected and desirable: the validator binds to the exact code actually executed.

After merge, the workflow's `push: main` trigger regenerates the bundle against the durable `main` commit. A PR artifact therefore proves the mechanism and workflow; the final production decision must use the bundle generated for the exact target-hardware measurement source revision.

## Permanent validation rules

The bundle validator fails closed on:

- missing candidates;
- unexpected top-level candidate/evidence entries;
- extra files within any candidate evidence directory;
- symlinked candidate directories or `full.json` evidence;
- candidate/path identity mismatch;
- mixed source commits;
- mismatch with an explicitly expected checkout commit;
- target version/data/digest drift;
- malformed numeric target identity, including booleans masquerading as integers;
- trace count/operation-count drift;
- trace fingerprint drift;
- SEM-surface drift;
- malformed Git-SHA or SHA-256 provenance identities.

The unsealed input directory contract is deliberately closed: before `bundle-manifest.json` is emitted, the input root must contain exactly the four production-candidate directories, and each directory must contain exactly one real `full.json` file.

## Sealed-bundle consumption contract

Creating a bundle and consuming a stored bundle are deliberately separate operations.

A sealed bundle contains the four candidate directories **plus** `bundle-manifest.json`. Downstream decision tooling must use the sealed-bundle validator rather than trusting the manifest or manually passing four filenames. The validator:

1. requires exactly the four production-candidate directories plus `bundle-manifest.json`;
2. rejects symlink indirection for the manifest, candidate directories and evidence files;
3. requires the manifest to have the exact closed schema;
4. recomputes `bundle_sha256` from the canonical manifest payload;
5. rechecks the frozen target, trace schema, SEM surface and candidate order;
6. optionally binds the bundle to an expected source commit;
7. independently reopens all four `full.json` records;
8. revalidates their target/commit/trace/evidence identities;
9. rehashes every child file and requires the manifest candidate entry to equal the independently recomputed entry exactly.

Permanent regressions cover sealed inventory drift, manifest-digest tampering, child-file tampering, expected-commit mismatch, and malicious candidate metadata with a recomputed top-level digest. This closes the distinction between “a manifest claims these four files” and “these exact four current files are the evidence that manifest sealed.”

## 2026-08-23 post-admission hardening

A static audit after the original admission identified one mismatch between documentation and enforcement: the record said the bundle contained **exactly** the four production candidates, but the validator previously required only that those four existed. Unrelated extra files or directories would therefore have been tolerated even though they were not incorporated into the bundle digest.

The validator now enforces the closed input inventory and sealed-consumption rules above. These changes narrow accepted evidence and strengthen downstream revalidation; they do not change the four-candidate generation path, evidence semantics, trace surface, or bundle digest algorithm for a canonical input tree.

Focused development checks confirm the canonical input shape passes and representative extra-directory, stray-file, extra-candidate-file and symlink substitutions fail closed. Permanent tests cover the closed input and sealed-bundle tamper classes.

Hosted Actions capacity became unavailable before the final hardened/documented head could rerun: affected jobs fail before checkout with zero recorded steps and no job logs. This is recorded as an infrastructure condition, not as a code PASS or code failure. The earlier implementation checkpoint remains the executed admission evidence for the bundle mechanism; the post-admission hardening must receive a normal hosted or equivalent local full rerun before its evidence is used for a final production-policy decision.

## Decision boundary

The bundle itself does not make any performance or production-policy decision. It closes the correctness-evidence handoff needed by the Pareto layer. Final selection still requires the exact same source revision to have:

1. a qualifying full correctness bundle;
2. controlled representative-population and synthetic target-hardware evidence;
3. a dimension-separated Pareto analysis;
4. an explicit human-reviewed production-policy record.
