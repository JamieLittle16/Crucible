# Helve branding and public presentation

This document defines the canonical public-facing language for Helve and the rules for using project branding in repository and community surfaces.

It is intentionally narrow. Architecture belongs in `docs/architecture/`; this file exists to stop the project name, motto, status and claims from drifting across README files, social previews and future websites.

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

Until a playable release exists, public surfaces MUST make the development status clear.

Preferred language at the current boundary:

> Helve has reached replay-free stock-client Play entry on Minecraft Java 26.2; R2C world/chunk/light projection is the next product boundary. Helve does not yet provide a playable production server release.

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

For supported clients, Helve SHOULD identify itself through the protocol-defined server-brand mechanism rather than impersonating the vanilla server.

For Minecraft Java 26.2 the canonical product brand string is exactly:

```text
Helve
```

Historical vanilla capture material may still contain `vanilla`; that is evidence and MUST NOT be rewritten. Product composition may replace the captured brand only after the source/capture artifact has been independently validated, and the resulting runtime packet must remain source-backed and fail closed on malformed identity or encoding.

## Visual identity

The **pixel-art family is canonical**. Do not smooth, vector-trace, repaint or replace it with the more rendered/non-pixel variants merely for convenience. Resizing raster exports should use nearest-neighbour sampling so the pixel geometry remains crisp.

Canonical repository exports live under `docs/assets/branding/`:

| Asset | Role |
| --- | --- |
| `helve-mark.png` | Primary standalone mark. Use for avatars, icons, compact square surfaces and places where the name is already visible. |
| `helve-lockup.png` | Primary horizontal identity. Use for the README/header and other wide project surfaces. |
| `helve-wordmark.png` | HELVE wordmark without the standalone mark. Use where a type-only treatment is required. |
| `helve-stacked.png` | Vertical mark + wordmark composition for square/tall layouts. |
| `helve-badge.png` | Secondary decorative badge. Use for social/community artwork or larger showcase surfaces; it is not the default everyday logo. |

The simpler hammer/anvil silhouette is the primary identity. The framed badge is intentionally secondary so the brand remains recognisable at small sizes and does not become visually overworked.

The committed PNGs are repository/web exports of the approved artwork. They are palette-optimised and resized with nearest-neighbour sampling to keep Git history and web delivery lightweight without changing the composition. Preserve higher-resolution masters outside derived raster exports where practical.

Do not repeatedly recompress an existing raster and treat it as a new master. New exports should be derived from the best available approved source artwork.

## GitHub presentation

The repository front page should prioritize, in order:

1. Helve horizontal pixel-art lockup and motto;
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

## Internal compatibility namespace

The public product name is Helve. Existing internal Cargo packages, crate imports and stable technical paths using the `crucible-*` namespace may remain during the first rebrand phase when renaming them would create dependency-graph churn without user-facing benefit.

That compatibility namespace must not leak into new public product copy. A later explicit migration may rename it if the engineering benefit justifies the churn.

Historical evidence identifiers, hashes, captures and durable technical records are provenance, not branding, and should not be mechanically rewritten merely to eliminate the old word from search results.

## Independence notice

Where legal/project identity context is appropriate, use:

> Helve is an independent project and is not affiliated with, sponsored by, or endorsed by Mojang Studios or Microsoft. Minecraft is a trademark of Microsoft Corporation.

Do not place this disclaimer above the product identity on every surface; it belongs in README/legal/footer contexts where it is useful and visible.

## Change discipline

Changes to the project name, motto, canonical short description, server-brand string or visual identity should be deliberate project decisions rather than incidental edits bundled into unrelated implementation PRs.

Presentation may evolve. Claims still require evidence.
