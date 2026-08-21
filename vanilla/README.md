# Vanilla Atlas

This directory contains **metadata, independent records, semantic classifications, generated reports, and provenance only**.

Do **not** commit Mojang source code or proprietary server artifacts.

The current local source corpus is Minecraft Java 26.2 and is pinned by `vanilla.lock.toml`. Tooling will index local source paths supplied by the developer and store only source identities/fingerprints and independent semantic records in this repository.

Planned commands:

```text
cargo xtask vanilla pin
cargo xtask vanilla index
cargo xtask vanilla show <symbol>
cargo xtask vanilla deps <symbol>
cargo xtask vanilla coverage <subsystem>
cargo xtask vanilla unknown
cargo xtask vanilla stale
cargo xtask vanilla frontier <milestone>
cargo xtask vanilla diff <old> <new>
```
