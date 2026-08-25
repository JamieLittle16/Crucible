# R1B Fresh-Player Play-Entry Source Review

Status: **bounded discovery review; not an admission gate**  
Tracker: #146  
Target: Minecraft Java 26.2 / protocol 776

## Purpose

Configuration completion in 26.2 does not merely flip a phase bit. The reviewed finish handler
installs the Play outbound protocol and immediately invokes the already-prepared spawn path; the
reviewed `PlayerList.placeNewPlayer` body then begins a substantial fresh-player bootstrap.

The Atlas discovery frontier intentionally over-approximates that body. It currently reaches 94
methods, but reachability is not evidence that every method or packet is mandatory on Crucible's
selected default profile. The final Play-entry gate must therefore be smaller than the frontier.

This review stage exists to answer one question before any final Play-entry VAR/SEM gate is written:

> Which source bodies are inseparable from a successful fresh-player Play entry on the selected
> 26.2 profile, and which are conditional, default-empty, delegated, or server-internal?

## Discovery evidence

The source-free Atlas discovery rooted in the already-reviewed `placeNewPlayer` body reports:

- 94 reachable methods;
- 57 root methods after expansion;
- 47 `CLIENT_OBSERVABLE` hazards;
- seven `CODEC` hazards;
- one `NETWORK_SEND` hazard;
- five `REGISTRY` hazards.

Those counts are triage data only. They are deliberately not copied into production packet law.

The exact reviewed `placeNewPlayer` body already proves five direct sends occur unconditionally before
helper-driven bootstrap work:

```text
ClientboundLoginPacket
ClientboundChangeDifficultyPacket
ClientboundPlayerAbilitiesPacket
ClientboundSetHeldSlotPacket
ClientboundUpdateRecipesPacket
```

It also directly reaches permission, recipe-book, scoreboard, teleport, player-info, level-info,
active-effects and inventory/menu helpers. Several of those helpers may be conditional or empty for a
fresh default player, so their source bodies must be inspected before admission.

## Bounded review plan

`vanilla/reviews/network/r1b-play-entry-source-review-plan.json` selects 27 exact source bodies for the
first source-rich pass. The set contains:

- the Play protocol registration surface needed to derive packet identity;
- `PlayerList.placeNewPlayer` itself;
- permission, recipe-book, scoreboard, level-info and active-effect helpers;
- both teleport layers reached by placement;
- conditional server-status and inventory/menu initialization;
- codec/construction bodies for the five direct packets;
- initial player-info publication;
- the player-position packet reached by teleport.

The first pinned selector preflight established that 26.2 `ClientboundPlayerPositionPacket` is a
record-backed packet whose wire law is declared by its static `STREAM_CODEC`; it has no packet-local
`write(FriendlyByteBuf)` body. The discovery plan therefore binds the packet `<clinit>()` and `of(...)`
construction helper and deliberately does not invent a nonexistent writer fingerprint. Review of the
static codec must enumerate any real subordinate codec bodies that remain material.

The plan is not a final gate. If a reviewed helper delegates to another body whose change could alter
the selected observable bootstrap, that dependency must be added before final Play-entry admission.
Conversely, helpers proved default-empty or irrelevant to the selected profile should not inflate the
final gate merely because vanilla calls them.

## Route dispositions

The source-free worksheet classifies each reviewed body as one of:

- `MANDATORY` — source-inseparable observable work on the selected route;
- `CONDITIONAL` — observable only when an explicit source condition is true;
- `DEFAULT_EMPTY` — helper is called but produces no selected-profile traffic when its input state is
  empty/default;
- `INTERNAL_ONLY` — server-side setup without client-observable packet law required by the R1 exit;
- `DELEGATED_REVIEW_REQUIRED` — the body is a wrapper and another exact body must be reviewed before
  classification can close.

This vocabulary is review evidence, not a runtime enum requirement.

## Source-text firewall

Run:

```text
python3 tools/r1b_play_entry_source_review.py \
  --output-dir /tmp/r1b-play-entry-source-review
```

The output directory must be fresh and outside the repository. It contains:

```text
review-dossier.json      source-rich; EPHEMERAL_DO_NOT_COMMIT
review-worksheet.json    source-free; blank dispositions
manifest.json            source/session identity
```

Selector preflight runs before any source excerpt is materialized. Every candidate must resolve
exactly once; same-arity overloads require an explicit exact Atlas signature. A failed prepare removes
its partial output.

This tool intentionally has **no finalize command** and emits no `VAR_REVIEWED` records or production
gate. Discovery review must first determine the minimum selected-route surface.

## Final admission sequence

After exact-body review:

1. classify the 27 discovery bodies and enumerate only material delegated dependencies;
2. write explicit Play-entry SEM rules for mandatory and conditional selected-route behavior;
3. create the minimum fingerprint-pinned VAR set and `GATE-NET-PLAY-ENTRY-26_2-001`;
4. bind exact clientbound Play packet identities from reviewed registration evidence rather than
   memory/community tables;
5. independently run the final gate against the pinned Atlas and require `admitted=true` with no
   failures;
6. exercise the resulting endpoint with an unmodified 26.2 client.

Chunks, light, ordinary movement and general gameplay remain outside this gate. The R1B exit is the
smallest real fresh-player Play bootstrap forced by Configuration completion, not a disguised whole
Play-protocol implementation.

## Performance boundary

The source review freezes observable behavior, not Mojang's allocation/object strategy. Production
Play bootstrap remains free to use shared immutable payload bodies, compact per-connection state,
bounded batching or vectored I/O where equivalence tests and controlled measurements support them.
The existing bounded publication/egress architecture remains the integration seam; this review must
not justify a second packet registry, session machine or outbound queue.
