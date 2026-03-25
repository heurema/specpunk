```
                       __
    ____  __  ______  / /__
   / __ \/ / / / __ \/ //_/
  / /_/ / /_/ / / / / ,<
 / .___/\__,_/_/ /_/_/|_|
/_/
```

**Spec-driven development for AI agents.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-64%20passing-brightgreen.svg)]()

> Your AI agent writes code. punk makes sure it writes the *right* code.

---

## The Problem

AI agents generate PRs that compile, pass tests, and silently break everything:

- **Scope creep** — asked to fix auth, also "improved" the database layer
- **Convention drift** — your style guide exists, nobody reads it
- **Ghost solutions** — old approach left in, new one piled on top
- **Review theater** — 1000-line PR, 3 reviewers, "LGTM"

The cost isn't generation. **It's verification.**

## How It Works

```
punk init          # scan your project in <10s — no LLM, no API calls
punk plan "task"   # generate a contract: scope, boundaries, acceptance criteria
punk check         # verify implementation stayed within the contract
punk receipt       # cryptographic proof of what happened
```

### `punk init` — Understand

Scans your codebase. Detects language, framework, conventions, test patterns, commit style, and never-touch boundaries. Machine-readable artifacts that make every AI interaction better.

### `punk plan` — Contract

Before code is written, punk creates a contract: what files to touch, what to never touch, acceptance criteria, and a quality score. You approve it like `terraform plan`. The contract becomes the review boundary.

### `punk check` — Verify

Compares the actual diff against the approved contract. Scope violations, never-touch breaches, undeclared files — caught in the pre-commit hook, not in code review.

```
punk check: FAIL (4 files checked, 3 in scope)

  error[NEVER_TOUCH]: .env
    .env is in project never_touch boundaries.
    fix: unstage this file or abandon the contract

  warning[UNDECLARED]: README.md
    README.md is not in contract scope.
    fix: `punk plan --expand` or `git restore --staged`
```

### `punk receipt` — Proof

Cryptographic receipt linking the approved contract to the verified diff. Receipt chain via SHA-256 hashes. Append-only. Auditable.

```
punk receipt: COMPLETED (3 files: +1 ~2 -0, 0 violations)
  contract: 7c0b9fb
  receipt:  .punk/contracts/7c0b9fb/receipts/task.json
```

---

## Quick Start

```sh
cargo install --path punk/punk-cli
```

```sh
cd your-project
punk init                               # scan project
punk plan --manual "add rate limiting"   # create contract, approve
# ... implement the feature ...
punk check                              # scope gate
punk check --strict                     # CI mode: undeclared = fail
punk receipt                            # completion proof
punk receipt --md                       # markdown summary
punk status                             # current state
punk close "changed requirements"       # abandon contract
```

## Design Principles

| Principle | What it means |
|-----------|---------------|
| **Brownfield-first** | Built for existing codebases, not greenfield demos |
| **CLI-native** | Terminal, CI, pre-commit hooks. No web UI, no vendor lock-in |
| **Deterministic** | `init` and `check` never call an LLM. `plan --manual` works offline |
| **Receipts, not trust** | Every decision recorded. Every verification reproducible |

## Features

- **Multi-language scanning** — Rust, TypeScript, Python, Go, Java, Ruby, PHP, C/C++
- **VCS-agnostic** — git and [jj](https://github.com/jj-vcs/jj) via trait abstraction
- **SHA-256 tamper detection** — contracts are hash-signed at approval time
- **Atomic writes** — receipt writes use tempfile+rename, safe under concurrency
- **Symlink defense** — `.punk/` directories and contract dirs reject symlinks
- **`deny_unknown_fields`** — contract JSON rejects injected fields
- **Rust-style errors** — actionable messages with fix suggestions
- **Pre-commit ready** — `punk check` runs in <200ms, exit codes for CI gates

## Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Pass (or deliberate quit) |
| `1` | Scope violation (never_touch, dont_touch, strict undeclared) |
| `2` | No contract found |
| `3` | Contract not approved or tampered |
| `4` | Internal error (parse, I/O) |

## Roadmap

| Phase | Status | What |
|-------|--------|------|
| 0-2 | **done** | Scaffold, `init`, `plan`, `config` |
| 3 | **done** | `check` — scope gate, never_touch, pre-commit |
| 4 | **done** | `receipt` — completion proof, receipt chain |
| 5 | next | Convention scan — tree-sitter, `AGENTS.md` generation |
| 6 | planned | Explain gate — human comprehension requirement |
| 7 | planned | CI mode — GitHub Action, SARIF output |
| 8+ | planned | Risk router, recall, multi-model audit, cleanup |

## Why "punk"

Your codebase has rules that nobody wrote down, conventions that nobody enforces, and boundaries that nobody respects. punk makes them explicit, verifiable, and non-negotiable.

## Sponsor

punk is built in the open by one developer. If it solves a real problem for you:

| Network | Address |
|---------|---------|
| **Ethereum / Arbitrum / BNB** | `0x1EB9b1dec7Ee036BE0BABE9B75AdaF6BD72f546C` |
| **Solana** | `HR1i9CFb8D1yGXkiu7CkdhCqBJvsc1hSRrTPLR3f7Hcq` |

USDC, USDT, ETH, SOL — any token works. Sponsors get early access + roadmap voice.

## License

MIT
