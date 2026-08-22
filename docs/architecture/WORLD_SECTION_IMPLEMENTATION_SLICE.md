# M0 World Section Implementation Slice

This is the first end-to-end application of the Crucible reconstruction method.

```text
OFFICIAL 26.2 SOURCE
      ↓
VAR review records
      ↓
SEM-WORLD-SECTION-*
      ↓
crucible-world-contract
      ↓
crucible-world-reference
      ↓
production representation laboratory
      ↓
EQUIV + vanilla fixtures + performance qualification
```

## Boundary discipline

`crucible-world-contract` owns semantic coordinates, summary obligations, conservative-query semantics, and the statically dispatchable section contract. It owns no palette, allocator, scheduler, networking, persistence, world generation, or ownership implementation.

`crucible-world-reference` is deliberately direct and boring. It stores all semantic cells explicitly and maintains independent numeric witnesses so incremental mutation bookkeeping can be checked against a full recomputation oracle. It is a correctness oracle, not a production memory design.

Production storage is a replaceable HOT engine component. The interface is generic and intentionally need not be object safe: static dispatch/monomorphization is preferred for the hot path. No global service lookup is introduced.

## Efficiency gate

Ordinary owner-local block get/set candidates are expected to demonstrate:

- no allocation in the steady-state common path;
- no lock or atomic authority check in production hot state;
- no registry/hash-map lookup for target state facts;
- no global chunk/world lookup once a section handle/view is resolved;
- no dynamic-dispatch requirement;
- no O(4096) housekeeping for a single mutation unless evidence proves it amortized and superior;
- bounded representation transitions with transition latency measured separately from steady state.

The generated target-state database will provide direct compact state facts. Worldgen and disk decode may construct sections through different representations and finalize into the live component.
