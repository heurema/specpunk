# Provider Deltas

Date: 2026-04-11
Status: initial delta log

Purpose:

- track relevant upstream movement
- decide whether `specpunk` should **adopt**, **wrap**, **compose**, **defer**, or **retire**
- keep adapter growth honest

## Label legend

| Label | Meaning |
|---|---|
| **adopt** | move local product behavior onto the upstream primitive directly |
| **wrap** | keep a thin adapter around the upstream primitive |
| **compose** | combine upstream capability with local correctness/proof policy |
| **defer** | useful direction, but not worth local work yet |
| **retire** | remove local machinery because upstream is now clearly better |

## Current delta decisions

| Area | Current upstream reality | Local action | Why |
|---|---|---|---|
| agent runtimes / handoffs | providers are investing heavily here | **wrap** | `specpunk` needs boundedness and proof, not a competing runtime |
| tool calling / built-in tools | upstream-native and improving | **wrap** | normalize usage instead of growing local tool orchestration |
| MCP-style external connectors | good provider-neutral boundary | **compose** | keep local policy, use MCP where it already fits |
| tracing / run inspection | increasingly upstream-native | **compose** | import evidence into local receipts/proofs rather than rebuilding tracing stacks |
| session / memory primitives | increasingly upstream-native | **defer** | use when stable; avoid expanding local memory platform assumptions |
| structured schema / grammar controls | increasingly upstream-native | **adopt** | use to reduce brittle free-text contract heuristics |
| multimodal / computer use | increasingly upstream-native | **defer** | not needed for the current core loop |
| custom universal provider runtime | local temptation only | **retire** | would duplicate vendor direction and create drift |
| broad provider-zoo shell UX | local temptation only | **retire** | violates one-face operator shell direction |

## Review rule

When upstream improves, the first question is:

> does this let `specpunk` delete or simplify local adapter logic?

If the answer is no, default to no architectural expansion.

## Current expectation for `punk-adapters`

`punk-adapters` should mostly own:

- translation
- normalization
- preflight
- bounded invocation glue
- explicit failure classification

It should not quietly accumulate:

- policy truth
- acceptance semantics
- proof semantics
- giant provider abstraction layers
