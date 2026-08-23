# Crucible branding and public presentation

This document defines the canonical public-facing language for Crucible and the rules for using project branding in repository and community surfaces.

It is intentionally narrow. Architecture belongs in `docs/architecture/`; this file exists to stop the project name, motto, status and claims from drifting across README files, social previews and future websites.

## Canonical identity

**Project name:** Crucible

**Motto:**

> **Same game. Different engine.**

**Canonical short description:**

> A high-performance, parity-focused Minecraft: Java Edition server engine written in Rust.

**Canonical product statement:**

> Strict supported vanilla fidelity. Independently designed internals. Measured performance.

The short description is suitable for repository metadata and compact project listings. The product statement is suitable for README/header material when there is enough space to preserve all three clauses.

## What the brand should communicate

Crucible should feel:

- technically serious rather than hype-driven;
- performance-focused without making unqualified benchmark claims;
- strict about correctness without sounding hostile to contributors;
- independent and original rather than a Rust translation of Mojang's server;
- ambitious about the eventual product while precise about the project's current stage.

The core contrast is always:

```text
same supported game semantics
            +
independently designed engine mechanisms
```

## Current-status language

Until a playable release exists, public surfaces MUST make the foundational status clear.

Preferred language:

> Crucible is in **M0 — Foundation and World Kernel Qualification** and does not yet provide a playable server release.

Do not use language such as `production-ready`, `drop-in replacement`, `fully vanilla compatible`, `faster than Paper/Folia/Pumpkin`, or a headline player-count/performance claim until the relevant qualification and release evidence exists.

When a claim becomes true, attach it to exact supported scope and evidence rather than silently replacing the conservative language with marketing shorthand.

## Performance claims

A public performance statement should identify enough context to be meaningful. At minimum, preserve the distinction between:

- wall-clock throughput;
- per-core efficiency;
- memory/working-set impact;
- tail latency;
- workload/semantic coverage;
- target hardware and configuration.

A benchmark chosen because it makes Crucible look good is not, by itself, a brand asset.

## Vanilla/parity language

Preferred terms:

- `supported vanilla semantics`;
- `parity-focused`;
- `source-backed qualification`;
- `reference implementation`;
- `equivalence evidence`.

Avoid implying that Mojang source is copied, vendored, translated or redistributed. The official implementation is an oracle for semantic reconstruction and qualification; Crucible's repository contains Crucible-owned code, models, records, fixtures and derived evidence.

## Visual assets

Canonical visual assets should live under:

```text
docs/assets/branding/
```

Recommended exports are:

```text
crucible-icon.*
crucible-wordmark.*
crucible-lockup.*
crucible-social-preview.*
```

If the design has distinct light/dark variants, suffix the filename explicitly rather than relying on a viewer to recolour it.

Keep a high-quality source/master outside derived raster exports where practical. Do not repeatedly recompress a social-preview or README raster and treat the result as a new master.

The existing approved visual design is authoritative. Do not invent a new colour palette, icon geometry or wordmark treatment merely to fit one repository surface; adapt layout/export dimensions while preserving the identity.

## GitHub presentation

The repository front page should prioritize, in order:

1. Crucible identity and motto;
2. one-sentence product definition;
3. current development status;
4. why the architecture is different;
5. evidence/qualification method;
6. contributor path;
7. licence and independence notice.

The README should not become a substitute for the architecture documentation. It is a front door that helps the right reader find the right deeper document.

Recommended repository topics include:

```text
minecraft
minecraft-server
rust
game-server
server
performance
voxel
simulation
networking
open-source
```

## Independence notice

Where legal/project identity context is appropriate, use the existing repository language:

> Crucible is an independent project and is not affiliated with, sponsored by, or endorsed by Mojang Studios or Microsoft. Minecraft is a trademark of Microsoft Corporation.

Do not place this disclaimer above the product identity on every surface; it belongs in README/legal/footer contexts where it is useful and visible.

## Change discipline

Changes to the project name, motto, canonical short description, or visual identity should be deliberate project decisions rather than incidental edits bundled into unrelated implementation PRs.

Presentation may evolve. Claims still require evidence.
