# Specpunk vs Beads / Gas Town / Gas City

Date: 2026-04-03
Status: active synthesis
Confidence: medium-high

## Question

What should `specpunk` learn from Steve Yegge's `beads`, `gastown`, and `gascity` stacks without losing its own correctness-focused identity?

## Sources

- Steve Yegge, "Gas Town: From Clown Show to v1.0" — article reviewed via browser extraction on 2026-04-03
- `~/contrib/steveyegge/beads/README.md`
- `~/contrib/steveyegge/beads/AGENT_INSTRUCTIONS.md`
- `~/contrib/steveyegge/gastown/README.md`
- `~/contrib/steveyegge/gastown/CONTRIBUTING.md`
- `~/contrib/steveyegge/gascity/README.md`
- `~/contrib/steveyegge/gascity/CLAUDE.md`
- `~/contrib/steveyegge/gascity/docs/getting-started/coming-from-gastown.md`

## Main finding

These are not one thing. They are three layers:

1. **Beads** — durable work and memory plane
2. **Gas Town** — product shell with one top-level operator face (`Mayor`)
3. **Gas City** — extracted primitives/config platform

`specpunk` is currently strongest as a **bounded correctness and execution substrate**. That is good. The risk is not that `specpunk` is too small; the risk is that it becomes a half-shell, half-platform system without a clear durable work plane.

## What they do better than us

### Beads
- Makes work first-class and durable
- Keeps long-horizon continuity visible
- Treats work graph and dependencies as an operational data plane

### Gas Town
- Reduces reading burden with one obvious top-level persona
- Hides worker noise behind a product shell
- Turns orchestration into a user experience, not just a tool stack

### Gas City
- States primitives explicitly
- Separates primitives from derived mechanisms
- Avoids hardcoded role taxonomy in core architecture

## What `specpunk` already does better

- Stronger bounded `Contract` discipline
- Stronger `Scope` and `allowed_scope` semantics
- Better explicit `gate` / `proof` orientation
- Clearer emphasis on correctness before trust
- Better potential to keep safety invariants in code instead of in prompt folklore

## Strategic implication

`specpunk` should not become a copy of Gas Town.

It should instead become:

- **Beads-lite durable work plane**
- **stronger correctness kernel**
- **simpler product shell than Gas Town**

## Priority gaps revealed by this comparison

1. `Work ledger` is still not one obvious canonical plane
2. `Operator shell` is still more mode-centric than person-centric
3. `Primitives` and `layer boundaries` are not explicit enough
4. `Autonomous reliability` is improving fast, but not yet boring
5. `Project intelligence` exists, but is not yet a coherent overlay system

## What not to copy

- role mythology (`Mayor`, `Polecats`, `Wasteland`, etc.)
- platform gigantism before core invariants are stable
- blanket "no heuristics in code" thinking for safety-critical invariants
- storage migrations before the ledger model itself is sharp

## Action

Use this synthesis as the root for the following research tracks:

- `2026-04-03-specpunk-identity-and-layering.md`
- `2026-04-03-specpunk-work-ledger.md`
- `2026-04-03-specpunk-primitives-and-derived-mechanisms.md`
- `2026-04-03-specpunk-one-face-operator-shell.md`
- `2026-04-03-specpunk-repo-fixture-matrix.md`
- `2026-04-03-specpunk-autonomous-loop.md`
- `2026-04-03-specpunk-project-intelligence.md`
