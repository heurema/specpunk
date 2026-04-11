# ADR: provider alignment by build/wrap/avoid

Date: 2026-04-11
Status: accepted

## Context

Major providers are converging on the same outer edge:

- agent runtimes
- managed or built-in tools
- tracing and observability
- session, memory, caching, and compaction primitives
- CLI and IDE operator workflows
- multimodal and tool-using agents

This creates a strategic risk for `specpunk`:

- if it keeps growing custom runtime, memory, or orchestration layers, it will drift into unnecessary complexity
- if it treats every provider capability as a reason to add a new kernel abstraction, the product will become harder to trust and harder to operate

At the same time, `specpunk` still has a clear differentiated role:

- bounded execution
- local repo safety
- VCS-aware isolation
- deterministic gate semantics
- receipts, proofs, and recovery

## Decision

`specpunk` will follow a **build / wrap / avoid** rule.

### Build

Build only what is local-trust-critical:

- bounded `Scope` and safety policy
- repo scan and source anchors
- integrity-check selection
- VCS-aware isolation and rollback
- `Receipt`, `DecisionObject`, and `Proofpack`
- deterministic gate semantics
- simple operator shell
- fixture matrix and reliability harnesses

### Wrap

Wrap provider-native capabilities instead of rebuilding them:

- agent runtimes
- tool calling and built-in tools
- tracing and observability
- session and memory primitives
- structured output controls
- multimodal and computer-use capabilities
- IDE and CLI integrations

### Avoid

Avoid growing the kernel into:

- a custom universal agent runtime
- a large internal memory platform
- free-text-heavy orchestration magic
- an over-smart patch/apply universe
- a parallel tracing/eval stack
- a role-heavy ontology with low reliability value

## Rules

### Rule 1: default to no architectural change

New provider capability does not automatically imply a new kernel abstraction.

Default answer:

> no architecture change unless the new capability clearly lets `specpunk` simplify itself

### Rule 2: prefer wrap over rebuild

If `specpunk` already approximates a provider-native primitive and the provider version is now stable and better, prefer:

1. wrap the provider primitive
2. keep local trust boundaries and proofs
3. remove the worse custom layer when practical

### Rule 3: complexity must buy reliability or boundedness

Any new subsystem must clearly improve at least one of:

- boundedness
- reliability
- inspectability
- operator simplicity

If it does not, it should not enter the kernel.

### Rule 4: structured over free-text

Critical-path behavior should prefer:

- structured repo anchors
- explicit policy
- typed evidence

over:

- prompt exclusions
- free-text orchestration heuristics
- implicit targeting magic

## Consequences

### Positive

- keeps `specpunk` aligned with provider direction without copying provider platforms
- protects the product from unnecessary ontology and runtime growth
- sharpens the value proposition around correctness, stewardship, and proof
- makes roadmap decisions easier: build, wrap, or avoid

### Negative

- some custom ideas must be cut even if they are interesting
- not every provider feature belongs in the shell or kernel
- contributors must be stricter about proving that a new abstraction is actually necessary

## Review policy

Once per month, review official provider updates using the checklist in:

- `docs/research/2026-04-11-provider-alignment-build-wrap-avoid.md`

Allowed outcomes of that review:

- no change
- small adapter update
- simplification of existing `specpunk` logic

Not allowed from review alone:

- adding a new kernel abstraction without separate architecture review

## References

- `docs/product/ARCHITECTURE.md`
- `docs/product/NORTH-ROADMAP.md`
- `docs/research/2026-04-11-provider-alignment-build-wrap-avoid.md`
