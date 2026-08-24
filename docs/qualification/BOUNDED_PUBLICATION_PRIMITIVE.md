# Bounded Publication Primitive

Status: **production candidate; semantic/resource equivalence under permanent Configuration lab, performance selection pending controlled evidence**  
Tracking: #146 / #143  
Predecessor: [`CONFIGURATION_PUBLICATION_LAB.md`](CONFIGURATION_PUBLICATION_LAB.md)

## Purpose

The Configuration publication laboratory established a class of outbound work that is deliberately
different from an atomic response to one inbound semantic action:

```text
small semantic decision commits
        ↓
large immutable ordered output already determined
        ↓
bounded progress over multiple service opportunities
```

The existing `ConnectionDriver::process_one_transactional` remains authoritative for naturally
bounded inbound-action/response transactions. This primitive does not relax that law.

## Production boundary

`crucible-publication-core` owns only:

```text
PublicationCursor        // one usize, next body not yet admitted
PublicationStep          // Complete | Queued { index, body_bytes }
publish_one(...)         // at most one borrowed body -> existing bounded egress
```

It intentionally does **not** own:

- an `Arc` or another sharing policy;
- a publication-image container;
- Minecraft packet identities or codecs;
- Configuration stage/negotiation state;
- framing or egress storage;
- socket I/O;
- compression;
- scheduling/fairness beyond one finite step per call.

The caller owns immutable publication bytes. This permits generated static slices, a composition
shared image, or a later qualified representation without changing the progression law.

## Commit law

For cursor position `i`:

1. if body `i` does not exist, return `Complete` and change nothing;
2. borrow body `i` without allocation or copying into semantic state;
3. ask the existing `ConnectionDriver` to queue exactly that body;
4. if wire validation or bounded-capacity admission fails, return the driver error and leave the
   cursor unchanged;
5. only after queue admission succeeds, advance the cursor to `i + 1` and return `Queued`.

The increment after queue admission is infallible. A successful slice lookup proves
`i < publication.len() <= usize::MAX`, therefore `i != usize::MAX` and `i + 1` cannot overflow. The
production primitive deliberately does not retain the laboratory prototype's post-admission
`checked_add` failure path, because an error after egress commit would violate exactly-once retry
semantics.

## Resource properties

The primitive itself:

- owns one machine word per connection;
- allocates nothing;
- copies no publication bytes into cursor state;
- performs no lookup by packet name/version/registry key;
- admits at most one frame body per call;
- uses the existing bounded framing/egress implementation as the only outbound queue.

Publication-image retention and sharing costs belong to the caller and must be measured with the
selected production representation.

## Qualification relationship

The existing Configuration publication lab remains the broad semantic/resource qualification. Its
lab-local immutable image now delegates progression to `crucible-publication-core`, so permanent
coverage for byte equivalence, rollback, exact-fit/over-capacity behavior, partial drains, resume,
cross-connection isolation and oversized frames exercises the production primitive rather than a
second progression implementation.

The primitive also carries narrow unit tests for its local invariants. Duplicate tests are kept only
where they make the production contract independently reviewable.

## Performance claim boundary

This change does not claim that shared immutable publication is faster on production hardware.
Hosted CI timing remains diagnostic only. Final selection of the shared Configuration image must use
the controlled full benchmark required by `PERFORMANCE_QUALIFICATION_STANDARD.md` and the
Configuration publication laboratory.

A future target-specific image must additionally be tied to admitted Minecraft 26.2 VAR/SEM/contract
evidence before `Target26_2` may use it.

## Exit for this primitive

The target-neutral primitive may leave draft only when:

1. workspace format/check/clippy/tests/rustdoc are green;
2. the existing Configuration publication laboratory and benchmark smoke run unchanged in semantic
   result while exercising this production code;
3. code review confirms no atomic inbound-response behavior changed;
4. no second outbound queue, runtime packet registry, ownership policy or target semantics entered
   this crate.

Controlled performance evidence is required before the Configuration shared-image mechanism is
called a selected optimization or integrated as the target's production publication representation.
