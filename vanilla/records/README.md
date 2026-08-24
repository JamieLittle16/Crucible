# Vanilla review records

This directory contains machine-readable source-review sidecars. They are the persistent tracking layer projected into the disposable SQLite Atlas by `cargo xtask vanilla sync-records`.

Records must be created from a built Atlas with `record-template` so they carry the exact normalized fingerprint for the pinned Minecraft source version. Ordinary records bind a source method. When source semantics live in static field initializers or static initializer blocks, first run `tools/vanilla_declaration_index.py`; Atlas then exposes the type's ordered static initialization as the reserved synthetic evidence node `<clinit>()`, which can be reviewed and fingerprint-pinned through the same record flow.

`<clinit>()` is an evidence projection, not a Mojang-authored source method. It exists so declaration-backed protocol facts are not attributed to unrelated methods or inferred from memory.

Prose reasoning belongs in the corresponding VAR/SEM documentation; these files exist to make status, provenance, staleness and evidence links queryable.

Do not mass-generate `VAR_REVIEWED` records. A source method or declaration node advances only when the stated review/evidence level is genuinely satisfied.
