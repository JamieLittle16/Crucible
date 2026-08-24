# R1B Configuration Semantic Contract — Minecraft Java 26.2

Status: **selected-route semantic draft complete; fingerprint-pinned VAR/source gate and Play-bootstrap closure pending**  
Target: Minecraft **26.2**, protocol **776**, data version **4903**  
Source archive SHA-256: `1e9bca3dff83cd83e7905f8810f1ec9899361fa2dc83fe893bb48beeb04df750`  
Fingerprint algorithm: `java-token-v2-literal-sensitive`

This contract freezes the first strict local-development Configuration route which follows the
already-admitted R1A Login path. It describes vanilla-observable ordering, packet/state law and
spawn-readiness dependencies. It deliberately does **not** reproduce Mojang's listener/task object
graph, and it does not admit optional Configuration branches that the selected profile does not use.

Production target code remains blocked until every relied-upon source declaration is represented by
reviewed VAR records and `GATE-NET-CONFIG-26_2-001` admits with no failures.

## Exact obligations

- **SEM-NET-R1B-001 — Configuration registration order.** The 26.2 serverbound Configuration
  protocol registers, in order: `client_information`, `cookie_response`, `custom_payload`,
  `finish_configuration`, `keep_alive`, `pong`, `resource_pack`, `select_known_packs`,
  `custom_click_action`, `accept_code_of_conduct`. The clientbound protocol registers, in order:
  `cookie_request`, `custom_payload`, `disconnect`, `finish_configuration`, `keep_alive`, `ping`,
  `reset_chat`, `registry_data`, `resource_pack_pop`, `resource_pack_push`, `store_cookie`,
  `transfer`, `update_enabled_features`, `update_tags`, `select_known_packs`,
  `custom_report_details`, `server_links`, `clear_dialog`, `show_dialog`, `code_of_conduct`.
  Combined with the already-admitted zero-based insertion-order dispatcher law, the selected route
  therefore uses serverbound IDs `client_information=0`, `custom_payload=2`,
  `finish_configuration=3`, `select_known_packs=7`; and clientbound IDs `custom_payload=1`,
  `finish_configuration=3`, `registry_data=7`, `update_enabled_features=12`, `update_tags=13`,
  `select_known_packs=14`. These numeric identities are not production-admitted until the R1B VAR
  gate binds the Configuration protocol declarations.

- **SEM-NET-R1B-002 — Configuration entry publication order.** On initial Configuration entry the
  server sends its brand custom payload, then server-links only when the configured link set is
  non-empty, then enabled features. It then begins registry synchronization. Optional
  code-of-conduct and server-resource-pack work occurs after registry synchronization only when the
  corresponding server policy is configured. The selected first Crucible profile has neither
  server links, code-of-conduct text nor a server resource pack.

- **SEM-NET-R1B-003 — client brand payload.** Configuration accepts the client's common custom
  payload packet. For the vanilla server listener used by this route, common custom-payload handling
  performs no state update. The independent selected-route capture observes `minecraft:brand` with
  value `vanilla`; Crucible may validate the admitted custom-payload envelope without introducing a
  gameplay dependency on that brand value.

- **SEM-NET-R1B-004 — client information replacement.** The Configuration listener begins with the
  default `ClientInformation` carried by the initial common-listener cookie. A serverbound
  `client_information` packet replaces the listener's current client-information value. Its payload
  is `language` (Minecraft UTF-8, max 16 Java UTF-16 units), signed byte view distance, enum chat
  visibility, boolean chat colors, unsigned-byte model customization, enum main hand, boolean text
  filtering, boolean allows-listing, and enum particle status, in that order. The final value is
  carried into the player/spawn handoff.

- **SEM-NET-R1B-005 — known-pack wire law.** The server begins registry synchronization by sending a
  list of `KnownPack` values. A `KnownPack` is three UTF-8 strings in order: namespace, id, version.
  The serverbound selected-known-packs response uses the same element law and is bounded to at most
  64 elements. For the selected default 26.2 composition, the independently observed requested list
  and client-selected list are both exactly `[minecraft:core@26.2]`.

- **SEM-NET-R1B-006 — known-pack decision.** Registry packing uses the server-requested known-pack
  set only when the client's response list equals the requested list exactly. Any other accepted
  response selects the empty known-pack set for packing. A selected-known-packs response received
  when no registry synchronization is active is invalid. Crucible must therefore retain this as
  explicit per-connection negotiation state; it may not assume all 26.2 clients select the requested
  set.

- **SEM-NET-R1B-007 — registry publication ordering.** After the known-pack decision, registry
  synchronization emits zero or more `registry_data` packets and then exactly one `update_tags`
  publication before the synchronization task completes. A registry-data packet contains a registry
  resource key followed by a list of packed registry entries. The update-tags packet maps registry
  keys to tag network payloads; each network payload maps tag identifiers to integer registry-id
  lists. The exact subordinate identifier/entry/count codecs used by production remain gated by the
  R1B VAR set.

- **SEM-NET-R1B-008 — selected strict local-development profile.** The first production R1B route is
  intentionally narrower than the full registered Configuration protocol. It has no server links,
  no code-of-conduct task and no server resource-pack task. Under the captured default-vanilla 26.2
  composition it publishes brand `vanilla`, enabled feature `minecraft:vanilla`, requests
  `minecraft:core@26.2`, observes an exact client match, then publishes 29 registry-data packets,
  one update-tags packet and finally the finish-configuration packet once spawn preparation is
  ready. This is a selected-route compatibility claim, not permission to silently ignore optional
  branches when a future Crucible profile enables them.

- **SEM-NET-R1B-009 — spawn readiness precedes finish.** `PrepareSpawnTask` is mandatory on initial
  Configuration. It resolves saved/default spawn state, obtains the target spawn position, requests
  the player-spawn chunk region with radius 3, waits for that chunk-load future, and then waits for
  entities in the same radius before constructing/spawning the player. Therefore the server may not
  send the final Configuration completion merely because registry publication ended: an equivalent
  prepared-spawn capability must be ready first. Crucible may satisfy this dependency from an
  already-loaded pregenerated world and does not need Mojang's task object or world-generation path.

- **SEM-NET-R1B-010 — finish-configuration handshake.** The clientbound and serverbound
  `finish_configuration` packets are unit packets and terminal in their respective Configuration
  protocols. The server sends clientbound finish only after the ordered Configuration work and
  spawn preparation required by the selected route are complete. The serverbound acknowledgement
  completes the JoinWorld stage.

- **SEM-NET-R1B-011 — Configuration acknowledgement enters real Play handoff.** Handling the
  serverbound finish acknowledgement installs the Play clientbound protocol, performs duplicate
  profile/admission checks, and immediately invokes the already-prepared spawn path using the
  accepted Login profile and latest `ClientInformation`. R1B therefore cannot claim success by only
  changing a coarse `SessionPhase` to Play; the minimum fresh-player bootstrap caused by this handoff
  must be source-admitted and exercised by the real-client probe.

- **SEM-NET-R1B-012 — selected independent capture convergence.** The successful unmodified 26.2
  offline/uncompressed capture observes the selected Configuration prefix as:

  ```text
  server -> client: custom_payload(brand=vanilla)
  server -> client: update_enabled_features([minecraft:vanilla])
  server -> client: select_known_packs([minecraft:core@26.2])
  client -> server: custom_payload(minecraft:brand=vanilla)
  client -> server: client_information(...)
  client -> server: select_known_packs([minecraft:core@26.2])
  server -> client: registry_data x29
  server -> client: update_tags
  server -> client: finish_configuration
  client -> server: finish_configuration
  ```

  Interleaving is constrained by the source task/response boundaries, not by the textual grouping
  above. The committed capture witness records 34 serverbound-facing clientbound Configuration
  frames (44,432 packet-body bytes) and four client-to-server Configuration frames before both
  directions enter Play. Capture evidence confirms one concrete selected composition; it does not
  replace source law for codecs, ordering or alternative branches.

## Implementation freedom after source admission

The following are explicitly implementation choices rather than vanilla semantic requirements:

- Mojang's `ConcurrentLinkedQueue<ConfigurationTask>` need not exist in Crucible;
- composition-stable brand/features/registry/tag bytes may be generated or prepared once as an
  immutable Configuration publication image when provenance and composition identity are bound;
- per-connection Configuration state may be a compact tagged enum containing negotiation stage,
  publication cursor, latest client information and accepted profile only;
- large ordered registry/tag publication may drain incrementally under bounded egress after the
  known-pack decision commits, while naturally bounded atomic responses retain the existing atomic
  outbound-admission law;
- pregenerated-world readiness may provide the prepared-spawn capability without implementing world
  generation.

Any optimized/shared publication must remain byte-equivalent to the admitted reference contract and
must pass the Configuration Publication Laboratory plus target-specific replay before production
admission.

## Remaining source/evidence closure

Before production Configuration code:

1. create fingerprint-pinned VARs for the exact protocol registrations, selected packet codecs,
   Configuration listener/task methods, spawn-preparation methods and the minimum Play handoff;
2. run `GATE-NET-CONFIG-26_2-001` against the pinned Atlas database and require `admitted: true` with
   no failures;
3. close the exact subordinate registry/tag/identifier codec dependencies used by the selected
   publication;
4. source-review `PlayerList.placeNewPlayer` far enough to freeze the minimum fresh-player Play
   bootstrap caused immediately by Configuration completion;
5. materialize a finite selected-route Configuration contract from source plus the independent
   capture and bind any generated immutable publication image to exact target/composition evidence;
6. only then extend `Target26_2` and the product server through Configuration/Play entry.

## Product integration seam

The R1A product composition merged in #160 is the required continuation point for R1B. Configuration
must extend the reusable `crucible-server` library composition that already owns the listener-scoped
session epoch and preserves coalesced post-Login bytes at the exact phase boundary. R1B must not
introduce a second socket/transport stack or move Configuration semantics into the executable shell;
`main.rs` remains listener/CLI policy while the library composition continues the same admitted
connection through Configuration and the source-required fresh-player handoff.
