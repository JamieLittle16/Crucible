# Helve branding and public presentation

This document defines the canonical public-facing language for **Helve** and the rules for using project branding in repository and community surfaces.

Helve is the new public identity of the project previously called Crucible. Historical evidence, sealed capture identifiers and provenance records may retain the old name where changing it would falsify or obscure the record. The first rebrand PR also deliberately retains the existing internal Rust crate/path namespace (`crucible-*`) to avoid a large, user-invisible build-graph churn; public product surfaces must use Helve.

Architecture belongs in `docs/architecture/`; this file exists to stop the project name, motto, status and claims from drifting across README files, social previews and future websites.

## Canonical identity

**Project name:** Helve

**Motto:**

> **Same game. Different engine.**

**Canonical short description:**

> A high-performance, parity-focused Minecraft: Java Edition server engine written in Rust.

**Canonical product statement:**

> Strict supported vanilla fidelity. Independently designed internals. Measured performance.

The short description is suitable for repository metadata and compact project listings. The product statement is suitable for README/header material when there is enough space to preserve all three clauses.

## What the brand should communicate

Helve should feel:

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

Until a playable production release exists, public surfaces MUST make the development status clear.

Preferred language for the current boundary:

> Helve is experimental. A stock Minecraft Java 26.2 client now reaches replay-free Play through Helve's source-backed networking/bootstrap path; native world/chunk/light projection is the next product milestone.

Do not use language such as `production-ready`, `drop-in replacement`, `fully vanilla compatible`, `faster than Paper/Folia/Pumpkin`, or a headline player-count/performance claim until the relevant qualification and release evidence exists.

When a claim becomes true, attach it to exact supported scope and evidence rather than silently replacing conservative language with marketing shorthand.

## Performance claims

A public performance statement should identify enough context to be meaningful. At minimum, preserve the distinction between:

- wall-clock throughput;
- per-core efficiency;
- memory/working-set impact;
- tail latency and variance;
- workload/semantic coverage;
- target hardware and configuration.

A benchmark chosen because it makes Helve look good is not, by itself, a brand asset.

## Vanilla/parity language

Preferred terms:

- `supported vanilla semantics`;
- `parity-focused`;
- `source-backed qualification`;
- `reference implementation`;
- `equivalence evidence`.

Avoid implying that Mojang source is copied, vendored, translated or redistributed. The official implementation is an oracle for semantic reconstruction and qualification; Helve's repository contains project-owned code, models, records, fixtures and derived evidence.

## Minecraft server brand

The server-visible brand advertised through Minecraft's source-backed `minecraft:brand` custom payload is:

```text
Helve
```

That value is product identity, not a vanilla-semantic claim. The project should not advertise `vanilla`, `Crucible`, or a captured reference-server brand on current Helve runtime paths.

## Visual assets

Canonical visual assets live under:

```text
docs/assets/branding/
```

New exports should use the Helve identity:

```text
helve-icon.*
helve-wordmark.*
helve-lockup.*
helve-social-preview.*
```

The previous Crucible assets are legacy and should be removed/replaced when the approved Helve files are supplied. Do not invent substitute logo geometry or a temporary visual identity merely to fill the gap.

If the design has distinct light/dark variants, suffix the filename explicitly rather than relying on a viewer to recolour it.

Keep a high-quality source/master outside derived raster exports where practical. Do not repeatedly recompress a social-preview or README raster and treat the result as a new master.

## GitHub presentation

The repository front page should prioritize, in order:

1. Helve identity and motto;
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

## Rename boundaries

The public rebrand and protocol-visible product identity happen immediately. A few identifiers are deliberately not rewritten mechanically:

- VAR, SEM, gate and capture identifiers whose text is part of durable evidence;
- historical milestone prose when it is explicitly describing an observation made under the old name;
- the internal `crucible-*` Rust crate/path namespace in this first rebrand PR;
- repository URLs until the GitHub repository itself is renamed after the PR lands.

These exceptions preserve provenance and keep the rename from becoming unrelated architectural churn. They must not leak into current user-facing runtime messages or new public documentation.

## Independence notice

Where legal/project identity context is appropriate, use:

> Helve is an independent project and is not affiliated with, sponsored by, or endorsed by Mojang Studios or Microsoft. Minecraft is a trademark of Microsoft Corporation.

Do not place this disclaimer above the product identity on every surface; it belongs in README/legal/footer contexts where it is useful and visible.

## Change discipline

Changes to the project name, motto, canonical short description, or visual identity should be deliberate project decisions rather than incidental edits bundled into unrelated implementation PRs.

Presentation may evolve. Claims still require evidence.
