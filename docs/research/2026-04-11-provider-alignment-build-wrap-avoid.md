# Provider Alignment: Build, Wrap, Avoid

Date: 2026-04-11
Status: active guidance
Priority: P1

## Question

How should `specpunk` evolve alongside the major model providers without drifting into unnecessary complexity?

## Short answer

`specpunk` should stay a **local-first correctness and stewardship layer** over provider-native agent capabilities.

That means:

- build the local safety and proof substrate
- wrap provider runtimes, tools, traces, and session primitives
- avoid building a parallel all-purpose agent platform

## External direction snapshot

Across OpenAI, Anthropic, and Google, the durable pattern is converging toward:

- agent runtimes instead of raw prompt loops
- managed or built-in tool stacks
- tracing, observability, and eval surfaces
- session, memory, caching, and compaction primitives
- CLI and IDE operator workflows
- multimodal and tool-using agents as first-class behavior

This means the market is standardizing the **intelligence runtime edge**.
`specpunk` should not try to outgrow that edge into a duplicate platform.

## Decision matrix

### Build

These are `specpunk` core responsibilities and should stay in-house:

| Area | Why |
|---|---|
| bounded `Scope` / `allowed_scope` | primary safety primitive |
| repo scan + source anchors | local repo truth cannot be outsourced |
| integrity-check selection | repo readiness and policy are local concerns |
| VCS-aware isolation and rollback | local mutation control is a kernel concern |
| `Receipt`, `DecisionObject`, `Proofpack` | provider traces are not enough for local proof |
| deterministic gate policy | accept/block semantics must remain local and inspectable |
| operator shell (`go`, `start`, `gate`, `status`) | simple one-face UX is a product differentiator |
| fixture matrix and reliability harnesses | `specpunk` must own its own reliability story |

### Wrap

These should be consumed through adapters instead of rebuilt:

| Area | Why |
|---|---|
| agent runtimes | providers are moving faster here than we can sustainably match |
| tool calling and built-in tools | better to normalize than to reimplement |
| tracing and observability | import into receipts/proofs instead of duplicating |
| session and memory primitives | use provider state/session features where possible |
| structured output / grammar / effort controls | these are now provider-native control surfaces |
| multimodal / computer-use capabilities | not a `specpunk` kernel problem |
| IDE and CLI integrations | integrate over them, do not try to out-shell them |

### Avoid

These are the main anti-patterns:

| Area | Why to avoid |
|---|---|
| custom universal agent runtime | duplicates provider direction and increases drift |
| large internal memory platform | duplicates session/state work already happening outside |
| free-text-heavy contract magic | brittle and repeatedly harms reliability |
| over-smart patch/apply sorcery | too much cleverness for too little safety |
| giant role mythology / ontology growth | increases operator burden without improving trust |
| custom eval/tracing universe | unnecessary if provider traces can be wrapped |

## Architecture rule

When a provider ships a stable primitive that `specpunk` already approximates:

1. prefer **wrap** over new **build**
2. keep local trust boundaries and proofs
3. remove the custom layer if it is clearly worse than the provider primitive

Default answer:

> no architectural change unless a new provider primitive clearly lets us simplify `specpunk`

## Monthly review checklist

Run this once per month using official sources only.

### Sources

- OpenAI docs / cookbook / official updates
- Anthropic release notes / official news
- Google Gemini API / Google developers blog / DeepMind blog

### Questions

#### 1. Runtime
- Did a provider ship a materially better agent runtime or handoff primitive?
- Are we duplicating that runtime in `specpunk`?

#### 2. Tools
- Did a provider improve tool calling, built-in tools, web/file/computer-use, or MCP-style integration?
- Can we wrap it instead of extending our own execution magic?

#### 3. Observability
- Did a provider improve traces, evals, run inspection, or debugging?
- Can receipts or proofpacks import that evidence instead of inventing a parallel stack?

#### 4. Session / memory
- Did a provider improve sessions, memory, caching, or compaction?
- Which custom `specpunk` memory assumptions should be reduced rather than expanded?

#### 5. Operator UX
- Did a provider improve CLI or IDE workflows?
- Should `specpunk` integrate rather than compete at that layer?

#### 6. Structured controls
- Did a provider improve schema control, grammar constraints, function calling, or reasoning controls?
- Can we reduce free-text contract heuristics because of it?

### Labels

Every observed change gets one label:

- `ignore`
- `watch`
- `wrap`
- `simplify`

Do **not** add a new kernel abstraction from a monthly review alone.

### Exit criteria

A good monthly review usually ends with one of:

- no change
- a small adapter update
- a simplification of existing `specpunk` logic

If the review ends with several new abstractions, the review failed.

## Practical implication for current roadmap

The safest direction is:

- strengthen the bounded correctness substrate
- simplify shell UX
- prefer structured repo anchors over free-text guidance
- use provider-native runtimes and tool/tracing primitives where available
- reject complexity that does not improve reliability or boundedness
