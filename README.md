```
                       __
    ____  __  ______  / /__
   / __ \/ / / / __ \/ //_/
  / /_/ / /_/ / / / / ,<
 / .___/\__,_/_/ /_/_/|_|
/_/
```

**Agent orchestration platform for solo founders running AI agent fleets.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)

---

## What Is Specpunk

Specpunk is a control plane for AI agents. It dispatches tasks to agents running on different models and providers (Claude, Codex, Gemini), tracks their work through structured receipts, enforces budgets, and gives the human operator a query interface instead of a dashboard.

- **CLI-first** - no web UI, no React, no PostgreSQL
- **Agent-first** - the primary operator is an AI agent, not a human at a dashboard
- **Built-in verification** - every agent's output can be scope-checked before acceptance
- **Solo founder scale** - one human, multiple AI agents, multiple projects

## Workspace

```
specpunk/punk/
  Cargo.toml          # workspace
  punk-core/          # verification library (shipped v0.1, frozen)
  punk-cli/           # `punk` binary — init, plan, check, receipt
  punk-orch/          # orchestration library (Phase 0, in progress)
  punk-run/           # `punk-run` binary — dispatch, status, goals (scaffold)
```

### punk (verification CLI, frozen)

Spec-driven development for AI agents. Scan a project, generate a contract, verify the implementation stayed within scope.

```sh
cargo install --path punk/punk-cli

punk init                               # scan project
punk plan --manual "add rate limiting"   # create contract
punk check                              # scope gate
punk receipt                            # completion proof
```

### punk-run (orchestration CLI, in progress)

Agent dispatch, receipt tracking, goal system. Currently Phase 0 scaffold.

```sh
cargo install --path punk/punk-run

punk-run status                         # show tasks and receipts (Step 0.3)
punk-run config                         # show configuration (Step 0.4)
```

See [ROADMAP-v2.md](docs/product/ROADMAP-v2.md) for the full implementation plan.

## Documentation

| Document | Purpose |
|----------|---------|
| [VISION.md](docs/product/VISION.md) | Product north star, core abstractions, CLI surface |
| [ARCHITECTURE.md](docs/product/ARCHITECTURE.md) | Executable spec: queue protocol, auth, budget, patterns |
| [ROADMAP-v2.md](docs/product/ROADMAP-v2.md) | Step-by-step implementation plan |

## License

MIT
