# punk

**Your codebase is not ready for AI agents. punk fixes that.**

AI agents write code fast. But they don't know your conventions, your boundaries, or your invariants. They generate PRs that compile, pass tests, and silently break everything you've built.

punk is a CLI that scans your project, extracts its implicit rules, and turns every task into a verifiable contract — before any code is written.

```
punk init          # understand the project (< 10 seconds)
punk plan "task"   # generate a contract, not code
punk check         # verify the implementation matches the contract
punk receipt       # cryptographic proof of what happened
```

---

## The problem

Every AI coding tool today starts from a blank context. They read your files, guess your patterns, and hope for the best. The result:

- Convention drift (your style guide exists, nobody reads it)
- Scope creep (asked to fix auth, also "improved" the database layer)
- Ghost solutions (old approach left in, new one added on top)
- Review theater (1000-line PR, 3 reviewers, "LGTM")

**The cost isn't generation. It's verification.**

## What punk does

**Phase 1 — Understand** (`punk init`)

Scans your codebase in under 10 seconds. No LLM, no API calls. Detects language, framework, conventions, test patterns, commit style, and boundaries. Outputs machine-readable artifacts that make every subsequent AI interaction dramatically better.

**Phase 2 — Contract** (`punk plan "add user auth"`)

Before anyone writes code, punk generates a contract: what files to touch, what to never touch, acceptance criteria, and a quality score. You approve it like `terraform plan`. The contract becomes the review boundary.

**Phase 3 — Verify** (`punk check`)

After implementation, punk verifies the change stayed within the contract. Scope violations, missing acceptance criteria, convention breaks — caught automatically, not in review.

**Phase 4 — Receipt** (`punk receipt`)

Cryptographic proof of what was planned, what was built, and whether they match. Append-only. Auditable. The missing link between "the AI wrote it" and "we verified it."

## Design principles

- **Brownfield-first.** Built for existing codebases, not greenfield demos.
- **CLI-native.** Works in your terminal, your CI, your pre-commit hooks. No web UI, no vendor lock-in.
- **Deterministic by default.** `punk init` and `punk check` never call an LLM. Only `punk plan` uses one, and `--manual` works offline.
- **Receipts, not trust.** Every decision is recorded. Every verification is reproducible.

## Status

**v0.1.0 — MVP complete.** Full loop works: init, plan, check, receipt, status, close.

- 64 tests, 0 clippy warnings, 3 rounds of adversarial QA
- Rust workspace (punk-core lib + punk-cli bin)
- git + jj support via VCS trait abstraction
- Multi-language scanning: Rust, JavaScript/TypeScript, Python, Go
- SHA-256 approval hash with tamper detection
- Atomic receipt writes, symlink defenses, `deny_unknown_fields`

## Install

```sh
cargo install --path punk/punk-cli
```

## Quick start

```sh
cd your-project
punk init                              # scan project (< 10s, no LLM)
punk plan --manual "add rate limiting"  # create contract, approve
# ... implement the feature ...
punk check                             # did it stay in scope?
punk check --strict                    # CI mode: undeclared = fail
punk receipt                           # completion proof
punk receipt --md                      # human-readable markdown
punk status                            # where am I?
```

For LLM-powered contract generation:
```sh
punk config set-provider anthropic https://api.anthropic.com/v1/messages sk-ant-...
punk plan "add rate limiting to the API"
```

## Sponsor

punk is built in the open by one developer. If this solves a real problem for you, consider sponsoring development.

| Network | Address |
|---------|---------|
| **Ethereum** | `0x1EB9b1dec7Ee036BE0BABE9B75AdaF6BD72f546C` |
| **Arbitrum** | `0x1EB9b1dec7Ee036BE0BABE9B75AdaF6BD72f546C` |
| **BNB Chain** | `0x1EB9b1dec7Ee036BE0BABE9B75AdaF6BD72f546C` |
| **Solana** | `HR1i9CFb8D1yGXkiu7CkdhCqBJvsc1hSRrTPLR3f7Hcq` |

USDC, USDT, ETH, SOL — any token on these networks works.

**What sponsorship enables:**
- Phase 5+ development (convention scan, AGENTS.md, CI mode)
- Multi-model contract generation (Claude + GPT + Gemini)
- Convention auto-tuning (auto-research loop for rule optimization)
- `punk scan --agents-md` — generate optimized AI agent instructions from your codebase

Every sponsor gets early access to features and a voice in the roadmap.

## Roadmap

| Phase | Status | What |
|-------|--------|------|
| 0 | done | Scaffold, CLI, VCS trait |
| 1 | done | `punk init` — brownfield scan, conventions, boundaries |
| 2 | done | `punk plan` — contract generation, quality heuristic, approval |
| 3 | done | `punk check` — scope gate, never_touch, pre-commit |
| 4 | done | `punk receipt` — completion proof, receipt chain |
| 5 | next | Convention scan — tree-sitter, AGENTS.md generation |
| 6+ | planned | Explain gate, CI mode, risk router, multi-model audit |

## Why "punk"

Because your codebase has rules that nobody wrote down, conventions that nobody enforces, and boundaries that nobody respects. punk makes them explicit, verifiable, and non-negotiable.

## License

MIT
