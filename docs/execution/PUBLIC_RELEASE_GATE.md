# Public Release Gate

Crucible becomes public only after the repository, history, CI and governance boundaries are independently safe. Public visibility is an operational change, not a substitute for a release gate.

## 1. Licensing gate

The MPL-2.0 + contributor-licensing framework landed in #52. Before visibility changes:

- keep MPL-2.0 as the public source licence;
- require affirmative CLA acceptance for external contributions;
- retain the explicit Mojang/Microsoft independence and trademark notice;
- do not accept third-party code before the contribution grant is effective.

## 2. Full-history artifact and credential audit

The current-tree `cargo xtask guard` is necessary but insufficient because a file deleted from `main` remains in Git history.

Before public launch, fetch every branch and GitHub pull-request head into the local clone, then run the repository-native history auditor from the final #53 head:

```bash
git fetch --prune origin '+refs/heads/*:refs/remotes/origin/*'
git fetch --force origin '+refs/pull/*/head:refs/crucible-audit/pull/*'
python3 tools/public_release_audit.py --repo-root .
```

The audit examines every reachable historical blob and fails on historical occurrences of:

- `mc-src.zip`;
- `.jar` server/runtime artifacts;
- `.crucible/`, `vanilla/source/`, `vanilla/artifacts/`, or `vanilla/private/` content;
- key stores, private-key files and `.env*` files;
- high-confidence GitHub/AWS/Google/Slack/private-key credential signatures.

Historical `.bootstrap/` transport payloads are reported as `REVIEW` rather than a security failure. Those payloads are not production repository content; the stale remote refs carrying them are removed after the PASS.

Temporary local pull-request audit refs can then be removed with:

```bash
git for-each-ref --format='delete %(refname)' refs/crucible-audit/pull | git update-ref --stdin
```

A PASS from the final auditor revision is required. A detected credential must be revoked or rotated before history remediation; deleting or rewriting Git history does not make an exposed credential safe again.

## 3. Branch/ref cleanup

The 2026-08-23 remote inventory contains 57 branches.

Preserve at public launch:

```text
main
m0/section-correctness-bundle
m0/section-final-qualification-driver
m0/section-pareto-raw-evidence-audit
m0/section-target-hardware-harness
m0/section-vanilla-fixtures
```

The first two `m0/` branches back open reviewed PRs #50 and #51. The final three contain unique, unsuperseded experimental work and are deliberately retained until that work is accepted, superseded, or abandoned. Their retention is explicit rather than accidental.

After #53 is merged and the final history audit has passed while all refs are still present, delete the following exact 51 cleanup-only branches:

```text
backup/docs-staging
backup/pre-atlas-main-cleanup
backup/pre-history-cleanup
backup/pre-rebase-pr53-2026-08-23
backup/pre-squash-pr52-2026-08-23
bootstrap/atlas-hardening-import
bootstrap/ci-docs
bootstrap/docs-import
bootstrap/docs-import-final
bootstrap/import-trigger
bootstrap/world-section-import
ci/foundation-hardening
ci/foundation-hardening-2
history/clean-main
history/clean-main2
legal/mpl-cla-public-release
m0/atlas-field-initializer-fix
m0/section-benchmark-corpus-import
m0/section-benchmark-harness
m0/section-full-qualification
m0/section-pareto-decision
m0/section-policy-candidates
m0/section-policy-decision-record
m0/section-population-outlier-guard
m0/section-qualification-harness
m0/section-real-corpus-probe
m0/section-representations
m0/section-representative-corpus
m0/section-representative-set
m0/section-runtime-fixture-evidence
m0/section-save-extractor
m0/section-semantic-fixtures
m0/section-target-combined-orchestrator-wip
m0/section-target-hardware-orchestrator-wip
m0/section-target-hardware-qualification
m0/section-target-synthetic
m0/section-vanilla-corpus
m0/stable-section-representations
m0/state-data
m0/state-data-binding
m0/state-data-finalization
m0/state-source-qualification
m0/state-source-qualification-tempcheck
m0/vanilla-atlas
m0/vanilla-atlas-hardening
m0/vanilla-atlas-work
m0/world-section-semantics
m0/26.2-qualified-state-data
ops/public-release-hardening
test/tree-content
test/tree-content2
```

A maintainer with a local clone can remove the exact set with:

```bash
git push origin --delete \
  backup/docs-staging \
  backup/pre-atlas-main-cleanup \
  backup/pre-history-cleanup \
  backup/pre-rebase-pr53-2026-08-23 \
  backup/pre-squash-pr52-2026-08-23 \
  bootstrap/atlas-hardening-import \
  bootstrap/ci-docs \
  bootstrap/docs-import \
  bootstrap/docs-import-final \
  bootstrap/import-trigger \
  bootstrap/world-section-import \
  ci/foundation-hardening \
  ci/foundation-hardening-2 \
  history/clean-main \
  history/clean-main2 \
  legal/mpl-cla-public-release \
  m0/atlas-field-initializer-fix \
  m0/section-benchmark-corpus-import \
  m0/section-benchmark-harness \
  m0/section-full-qualification \
  m0/section-pareto-decision \
  m0/section-policy-candidates \
  m0/section-policy-decision-record \
  m0/section-population-outlier-guard \
  m0/section-qualification-harness \
  m0/section-real-corpus-probe \
  m0/section-representations \
  m0/section-representative-corpus \
  m0/section-representative-set \
  m0/section-runtime-fixture-evidence \
  m0/section-save-extractor \
  m0/section-semantic-fixtures \
  m0/section-target-combined-orchestrator-wip \
  m0/section-target-hardware-orchestrator-wip \
  m0/section-target-hardware-qualification \
  m0/section-target-synthetic \
  m0/section-vanilla-corpus \
  m0/stable-section-representations \
  m0/state-data \
  m0/state-data-binding \
  m0/state-data-finalization \
  m0/state-source-qualification \
  m0/state-source-qualification-tempcheck \
  m0/vanilla-atlas \
  m0/vanilla-atlas-hardening \
  m0/vanilla-atlas-work \
  m0/world-section-semantics \
  m0/26.2-qualified-state-data \
  ops/public-release-hardening \
  test/tree-content \
  test/tree-content2
```

Do not run that deletion command until #53 is merged and the final audit PASS has been recorded. The backup refs deliberately remain reachable until the audit has inspected them.

After launch, enable GitHub **Automatically delete head branches** so merged branch accumulation does not recur.

## 4. `main` protection

Issue #39 remains the authority for repository rules. GitHub must enforce on `main`:

- pull request required before merge;
- `Rust and tooling quality` required;
- `Repository guard` required;
- `Contributor licence agreement` required;
- branch required to be up to date before merge;
- conversation resolution required;
- force pushes blocked;
- deletion blocked;
- routine bypass of required checks disabled;
- squash merge as the normal history policy.

Create and activate this rule before public visibility when the current GitHub plan exposes private-repository rulesets/branch protection. If the current plan only exposes these controls after a repository becomes public, visibility change and rule activation form one uninterrupted admin checkpoint: do not announce the repository or accept contributions between them.

Do not require a ceremonial second approval while Crucible has one active maintainer. Add CODEOWNERS/review-count rules when that becomes a real governance boundary.

## 5. Public-fork Actions posture

Public fork PRs are untrusted input. Repository workflow law is:

- never use `pull_request_target` to execute contributor-controlled code;
- default workflow permissions to read-only;
- never expose repository secrets to untrusted PR code;
- checkout with persisted credentials disabled;
- pin third-party Actions by immutable full commit SHA;
- keep target-hardware performance evidence separate from hosted-CI timing.

Ordinary correctness CI may run on external PRs. Heavy official-runtime/world-generation qualification should remain maintainer-controlled when abuse or concurrency pressure becomes material; hosted timing remains non-decision evidence regardless.

In GitHub Actions settings, retain approval controls for first-time or untrusted fork contributors rather than automatically executing arbitrary fork workflows without review.

## 6. Vulnerability reporting

`SECURITY.md` defines the public reporting policy. GitHub private vulnerability reporting is a public-repository feature, so enable it immediately after the visibility change, before Crucible is announced or treated as open for unsolicited contributions.

## 7. Visibility change order

The final sequence is:

1. #52 licensing/CLA landed;
2. locally validate the final #53 head;
3. fetch all branch and PR refs and obtain a final full-history audit PASS;
4. merge #53 by squash;
5. delete the 51 cleanup-only branches above;
6. verify the six intentionally retained branches are the complete remote inventory;
7. enable automatic merged-head deletion and verify Actions fork-approval / least-privilege workflow settings;
8. if available while private, activate the `main` ruleset from #39 with all three required checks;
9. change repository visibility to public;
10. if the ruleset could not be activated while private, activate it immediately now;
11. immediately enable Private vulnerability reporting;
12. disable merge-commit/rebase merge methods if the repository is to enforce squash-only history through repository settings;
13. verify `main` protection is active and the repository reports MPL-2.0 correctly;
14. dispatch/observe normal public hosted CI and require it to pass;
15. verify README and SECURITY rendering plus the external CLA failure path;
16. only then announce Crucible or treat it as open for unsolicited contributions.

If any gate is uncertain, stop the transition before the repository is announced or contributions are accepted. A brief public-but-not-yet-announced admin interval is acceptable when GitHub only exposes a required control after visibility changes.
