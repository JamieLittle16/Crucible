# R1B fresh-player Play-entry source discovery

**Target:** Minecraft Java 26.2 / protocol 776  
**Parent:** #146  
**Status:** discovery frontier only; no Play packet law admitted

## Purpose

The reviewed Configuration finish path does not end at a generic phase transition. The pinned
`ServerConfigurationPacketListenerImpl.handleConfigurationFinished` path installs the Play outbound
protocol and immediately invokes the prepared fresh-player spawn path. The exact reviewed
`PlayerList.placeNewPlayer` body then performs a substantial initial Play bootstrap.

`vanilla/frontiers/r1b-play-entry-selected.json` exists to discover the minimum source-inseparable
Play surface that must be admitted before R1B can honestly claim that an unmodified 26.2 client has
entered Play.

This frontier is deliberately **discovery-only**. A root appearing here does not establish a packet
ID, codec, field law, unconditional emission, ordering guarantee or Crucible implementation shape.
Those claims require exact fingerprint-pinned VAR review, SEM extraction and independent source
admission.

## What the reviewed handoff already establishes

The exact `PlayerList.placeNewPlayer` body directly constructs/sends or invokes paths involving:

- `ClientboundLoginPacket`;
- `ClientboundChangeDifficultyPacket`;
- `ClientboundPlayerAbilitiesPacket`;
- `ClientboundSetHeldSlotPacket`;
- `ClientboundUpdateRecipesPacket`;
- player permission publication;
- initial recipe-book publication;
- scoreboard synchronization;
- teleportation and level information;
- optional server-status publication;
- `ClientboundPlayerInfoUpdatePacket` initialization;
- active effects and inventory/menu initialization.

That source evidence is sufficient to establish that the Play bootstrap is source-inseparable from
Configuration completion. It is **not** sufficient to admit the subordinate packet codecs or to
assume every helper emits packets on the selected empty/default server profile.

## Discovery categories

The review should keep three categories separate.

### 1. Direct packet constructions

Packet classes directly constructed in the reviewed `placeNewPlayer` body are high-priority roots.
Their registration identity, constructor field law and clientbound codec must still be fingerprint
reviewed before use in `Target26_2`.

### 2. Helper-driven publication

Methods such as `sendPlayerPermissionLevel`, `updateEntireScoreboard`, `sendLevelInfo`,
`sendActivePlayerEffects`, teleport and initial recipe-book publication may hide additional packet
surfaces. Atlas should enumerate those dependencies first; exact helper bodies are then reviewed to
separate mandatory output from implementation-only work.

### 3. Conditional or default-empty branches

Scoreboards, statistics, recipe-book state, status data, effects and similar helpers may emit nothing
or less on the selected fresh/default server profile. Source control flow plus capture evidence must
choose the selected route. Registration alone is never evidence of emission.

## Scope boundary

Chunks, light, movement and general entity/gameplay breadth are intentionally excluded. R1B needs
the minimum fresh-player Play bootstrap required by the same Configuration handoff; chunk/light
publication is the following milestone and must not inflate this source gate.

The intended evidence split is therefore:

```text
GATE-NET-CONFIG-26_2-001
    direct Configuration spine
              +
GATE-NET-CONFIG-CLOSURE-26_2-001
    delegated Configuration dependencies
              +
GATE-NET-PLAY-ENTRY-26_2-001
    minimum source-inseparable fresh-player Play bootstrap
              ↓
Target26_2 Configuration -> admitted Play entry
```

The Play-entry gate name above is the planned admission boundary, not evidence that such a gate is
already reviewed or green.

## Local Atlas discovery

After updating `main`, the pinned local Atlas can enumerate this frontier without copying official
source bodies into Git:

```bash
cd ~/Crucible

git pull --ff-only

python3 tools/vanilla_atlas.py \
  --db .crucible/vanilla/atlas.sqlite \
  frontier r1b-play-entry-selected --json \
  > /tmp/r1b-play-entry-frontier.json

python3 tools/vanilla_atlas.py \
  --db .crucible/vanilla/atlas.sqlite \
  next r1b-play-entry-selected --limit 80 \
  > /tmp/r1b-play-entry-next.txt

python3 tools/vanilla_atlas.py \
  --db .crucible/vanilla/atlas.sqlite \
  deps "net.minecraft.server.players.PlayerList#placeNewPlayer(final Connection connection , final ServerPlayer player , final CommonListenerCookie cookie)" \
  > /tmp/r1b-play-entry-place-new-player-deps.txt
```

These are source-free structural reports. They are discovery inputs only. The next review step should
use them to choose a narrow exact-method dossier and then generate fingerprint-pinned records for
`GATE-NET-PLAY-ENTRY-26_2-001`.

## Required review questions

Before the Play-entry gate can be admitted, the source review must establish:

1. which direct `placeNewPlayer` packets are unconditional on the selected fresh-player route;
2. their exact 26.2 registration identities and wire codecs;
3. which helper calls actually emit output for the selected default server/player state;
4. the relative ordering that is client-observable and required for a successful vanilla-client
   bootstrap;
5. which apparent work is server-internal state setup rather than network semantics;
6. which branches are capture-dependent because source permits multiple policy/state outcomes;
7. the smallest observable endpoint that legitimately qualifies as “entered Play” before chunk/light
   work begins.

## Qualification direction

The eventual implementation should first have a simple source-backed reference bootstrap. Production
code may then share immutable payloads, preframe stable bodies, batch bounded publication or use
vectored I/O if whole-path measurements justify it. Those optimizations must preserve exact ordering,
backpressure and state-transition semantics.

CI benchmark smokes are regression diagnostics only. The selected publication mechanism must earn
performance claims on controlled hardware with representative fresh-player joins, including retained
shared bytes, per-connection allocations/copies, publication latency and throughput.

## Exit condition

This discovery stage is complete when Atlas output has been reviewed into a finite selected-route
method/packet set. R1B itself remains incomplete until that set has reviewed VAR/SEM evidence, an
independent green Play-entry source gate, parity/reference qualification, and a real unmodified 26.2
client probe reaching the admitted Play endpoint.
