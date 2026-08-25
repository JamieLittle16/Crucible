# R1B Fresh-Player Play-Entry Follow-up Source Review

Status: **bounded second-pass review; not an admission gate**  
Tracker: #146  
Target: Minecraft Java 26.2 / protocol 776

## Why this pass exists

The first exact-source Play-entry review classified 27 bodies: 22 mandatory, one conditional,
one default-empty, two delegated-review-required and one outbound-irrelevant decoder. The subsequent
source-free Atlas probe resolved the remaining helper signatures and packet surfaces without opening
or redistributing the official source.

This follow-up selects **35 exact source bodies**. It is intentionally much smaller than the
94-method discovery frontier. Inclusion means a body can change the selected fresh/default player's
observable bootstrap, or is needed to prove that a candidate branch is empty. Reachability alone is
not admission evidence.

## Included surface

The plan covers only:

- permission command-tree publication and its packet encoding;
- the permission entity event;
- the two always-emitted recipe-book packets;
- border, full clock sync, default spawn, mandatory game event and tick-rate synchronization;
- the delegated active-effect loop solely to prove selected-profile emptiness;
- delegated inventory-menu synchronization and its primary content packet;
- `CommonPlayerSpawnInfo` construction/wire law used by Login;
- composition-stable synchronized recipe data accessors;
- the player-info action table;
- position/relative subordinate codecs used by the fresh teleport;
- one-argument player-info broadcast routing;
- the already-conditional server-data packet codec.

The plan deliberately excludes scoreboard packet machinery because the first review classified the
selected fresh/default scoreboard as empty. It also excludes `ClientboundUpdateMobEffectPacket`: that
codec is admitted only if the reviewed active-effect loop shows the selected profile can actually emit
one during R1 entry.

## Architectural boundary

This review freezes observable behavior; it does not bless Mojang's representation. In particular:

- no runtime dynamic Play packet registry is required;
- command/recipe/registry structures may be precomputed or compactly materialized where their
  provenance and selected state are bound;
- packet publication continues through Crucible's existing bounded egress path;
- no second outbound queue or second session machine is justified by this review;
- selected-profile emptiness is preferable to importing broad subsystems that do not publish bytes.

## Source firewall

Run the tool only against the pinned local Atlas/source:

```text
python3 tools/r1b_play_entry_followup_source_review.py \
  --output-dir /tmp/r1b-play-entry-followup-source-review
```

The output directory must be fresh and outside the repository. It contains:

```text
review-dossier.json      source-rich; EPHEMERAL_DO_NOT_COMMIT
review-worksheet.json    source-free; blank dispositions
manifest.json            source/session identity
```

Selector preflight runs for all 35 bodies before source excerpts are emitted. Same-arity/semantic
overloads that matter are pinned by exact Atlas signature.

## Exit from this review

After exact-body inspection:

1. classify all 35 bodies with the same route-disposition vocabulary as the first pass;
2. follow only any still-material delegates that could alter selected-route output;
3. write the finite Play-entry SEM contract;
4. create the minimum fingerprint-pinned `VAR_REVIEWED` set and
   `GATE-NET-PLAY-ENTRY-26_2-001`;
5. run that gate independently against the pinned Atlas and require `admitted=true`;
6. only then admit production `Target26_2` Configuration-to-Play implementation.

Chunks, light, ordinary movement, persistence-rich scoreboard state, active effects absent from the
selected fresh profile, and general gameplay remain outside R1B.
