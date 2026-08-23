# Protocol Session-State Qualification

Issue: #86

## Purpose

`crucible-session-core` freezes the target-version-agnostic connection lifecycle law before Minecraft 26.2 packet IDs are bound to handlers.

The crate deliberately contains no packet registry, protocol-version constant, authentication policy, socket/runtime handle, callbacks or transition history.

## Admitted lifecycle

```text
Handshake
  ├─> Status
  └─> Login -> Configuration -> Play

(any non-closed phase) -> Closed
```

`Closed` is terminal. Closing an already-closed session is idempotent. All other repeated, skipped and backward transitions fail closed and leave the current phase unchanged.

## Integration law

A versioned packet handler may request a transition only **after** the source-backed packet has decoded and validated successfully. Packet decoding failure must therefore occur before calling `SessionState::advance`.

Target packet IDs and field layouts remain outside this crate.

## Permanent qualification

The crate-local suite covers:

1. every ordered phase pair against an independent admitted-edge matrix;
2. both complete status and play lifecycle histories;
3. close from every phase;
4. idempotent closure and rejection of every reopen attempt;
5. no partial state change after failed transitions;
6. a 100,000-attempt deterministic adversarial transition corpus.

Ordinary workspace CI runs strict formatting, all-target checking, Clippy with warnings denied, the full Rust test suite and rustdoc over this crate.

## Performance posture

Session transitions are cold-path control state. The implementation is intentionally one small enum field with direct matching. No runtime registry, heap state or dynamic dispatch is justified here. Optimization effort belongs in frame/packet I/O and later measured HOT gameplay paths, not in obscuring a handful of lifecycle transitions.
