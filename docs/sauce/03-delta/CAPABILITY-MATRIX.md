# Provider Capability Matrix

Date: 2026-04-11
Status: working matrix

Purpose:

- show where upstream-native capability already exists
- show where `specpunk` should **build**, **wrap**, or **avoid**
- keep `punk-adapters` thin and explicit

## Decision legend

| Label | Meaning |
|---|---|
| **build** | local kernel/substrate responsibility |
| **wrap** | consume through adapters instead of rebuilding |
| **compose** | combine local policy with upstream capability |
| **avoid** | do not grow local machinery here |

## Matrix

| Capability area | OpenAI / Anthropic / Google direction | MCP role | `specpunk` posture | Notes |
|---|---|---|---|---|
| agent runtime / handoff | strong upstream movement | not primary | **wrap** | do not build a parallel universal runtime |
| built-in tools / tool calling | strong upstream movement | connector boundary | **wrap** | normalize results, do not reimplement tool stacks |
| tracing / observability | strong upstream movement | optional bridge | **wrap** | import useful evidence into receipts/proofs |
| session / memory / compaction | strong upstream movement | n/a | **wrap** | reduce local assumptions when providers expose stable primitives |
| structured output / grammar / reasoning controls | strong upstream movement | n/a | **wrap** | use upstream controls to reduce free-text heuristics |
| multimodal / computer use | strong upstream movement | may connect tools | **wrap** | not a kernel problem |
| provider-neutral external connectors | mixed by provider | strong fit | **compose** | MCP is the preferred neutral boundary where it fits |
| repo scan + source anchors | local repo truth only | n/a | **build** | must remain local and inspectable |
| bounded scope / `allowed_scope` | local safety only | n/a | **build** | primary safety primitive |
| VCS isolation / rollback | local mutation control only | n/a | **build** | kernel responsibility |
| deterministic gate policy | local acceptance law only | n/a | **build** | must stay local |
| `Receipt` / `DecisionObject` / `Proofpack` | provider traces insufficient | n/a | **compose** | local truth plus wrapped upstream evidence |
| giant provider taxonomy / provider-zoo shell | tempting but harmful | n/a | **avoid** | grows complexity without strengthening trust |

## Current adapter implication

Today the active adapter ports should stay narrow:

- `ContractDrafter`
- `Executor`

Future advisory-only council work may also use:

- `ProviderAdapter`

Anything beyond that needs a strong proof that it simplifies the system rather than expanding it.
