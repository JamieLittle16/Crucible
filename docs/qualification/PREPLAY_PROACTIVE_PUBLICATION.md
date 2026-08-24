# Pre-play Proactive Publication Boundary

Status: **production networking primitive**  
Scope: target-neutral post-commit outbound progression before Play

## Purpose

Crucible already has two different outbound laws and they must remain different:

1. an inbound packet may produce a naturally bounded response batch, admitted atomically with that
   inbound frame by `PrePlayConnection::process_one`; and
2. a semantic decision may already be committed while a larger immutable publication still needs to
   enter bounded egress over multiple service opportunities.

Minecraft 26.2 Configuration contains the second shape. Registry/tag publication is much larger than
one ordinary response transaction, and the server also has proactive output which is not triggered by
an immediately corresponding inbound packet.

`PrePlayPublisher` is the narrow target-neutral seam for that second case.

## Production shape

```text
immutable target/composition publication
               +
       copied PublicationCursor
               +
        Copy commit token
               |
               v
      PrePlayPublisher::publication
               |
               v
 PrePlayConnection::service_publication
               |
               v
crucible_publication_core::publish_one
               |
               v
     existing ConnectionDriver egress
               |
       success only
               v
 PrePlayPublisher::commit_publication
```

The target never receives `ConnectionDriver` and cannot bypass bounded framing/egress admission.

## Commit law

For a proposed publication step:

- target context, session and target-local state are observed immutably;
- publication bodies are borrowed rather than transferred;
- the live cursor is represented only by a copied `PublicationCursor`;
- the commit token must implement `Copy`, so it cannot become a hidden owner of a `Vec`, `Box`,
  `Arc`, queue or publication image;
- `crucible-publication-core::publish_one` performs at most one frame admission;
- wire rejection or egress backpressure leaves target state unchanged;
- after `PublicationStep::Queued`, the advanced cursor and commit token are adopted infallibly; and
- `PublicationStep::Complete` may also be committed, allowing the target to leave a publication stage
  without manufacturing a packet or introducing a second state machine.

The generic `SessionState` is not changed by proactive publication. Session transitions remain owned
by the existing inbound semantic transaction path.

## Why this is an optional trait

`PrePlayPublisher` deliberately extends rather than enlarges `PrePlayTarget`.

Status and Login are naturally inbound-driven. Requiring every target to carry publication methods or
state would put an irrelevant branch into already-small paths and would blur the distinction between
atomic response batches and post-commit progression.

Only targets which genuinely have proactive work opt in.

## Relationship to bounded publication qualification

This seam does not reimplement publication progression. The queue/cursor law remains
`crucible-publication-core::publish_one`, graduated from the Configuration publication laboratory.
The binder contributes only the ownership/transaction rule around that primitive:

```text
proposal is not state
queue success precedes state
failure commits nothing
```

Permanent binder tests cover:

- idle/non-ready phases;
- exact one-body admission;
- egress backpressure preserving cursor, token and existing egress;
- wire rejection preserving cursor and target state;
- explicit zero-byte `Complete` stage commitment; and
- terminal sessions refusing proactive output.

## R1B intended use

After `GATE-NET-CONFIG-26_2-001` admits the exact Configuration source law, `Target26_2` may use this
seam to publish the generated/qualified Configuration bodies and progress a compact target-local
Configuration state.

This document does **not** admit:

- any Minecraft 26.2 Configuration packet ID;
- any packet field layout;
- any registry/tag contents;
- any selected publication-image ownership representation;
- any scheduling policy beyond one body per explicit service opportunity; or
- any Play/bootstrap behavior.

Those remain independently gated by source review, finite protocol materialization, product
composition, and real-client qualification.

## Performance boundary

The API requires no allocation on the service path. It introduces no second egress queue, no trait
object, no runtime target registry and no cloning of publication bodies. The target-specific commit
token is `Copy`; publication storage stays with its existing owner.

No hosted-CI timing result from this primitive is a mechanism-selection claim. If competing
Configuration image representations remain after source admission, they must still be qualified on
controlled target hardware under the existing Performance Qualification Standard.
