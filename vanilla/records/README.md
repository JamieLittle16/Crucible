# Vanilla review records

This directory contains machine-readable source-review sidecars. They are the persistent tracking layer projected into the disposable SQLite Atlas by `cargo xtask vanilla sync-records`.

Records must be created from a built Atlas with `record-template` so they carry the exact normalized method fingerprint for the pinned Minecraft source version. Prose reasoning belongs in the corresponding VAR/SEM documentation; these files exist to make status, provenance, staleness and evidence links queryable.

Do not mass-generate `VAR_REVIEWED` records. A method advances only when the stated review/evidence level is genuinely satisfied.
