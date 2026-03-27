# punk

Cargo workspace with 4 crates:

| Crate | Type | Status | Binary |
|-------|------|--------|--------|
| `punk-core` | library | shipped v0.1, frozen | - |
| `punk-cli` | binary | shipped v0.1, frozen | `punk` |
| `punk-orch` | library | Phase 0 scaffold | - |
| `punk-run` | binary | Phase 0 scaffold | `punk-run` |

## Install

```sh
cargo install --path punk-cli    # verification CLI
cargo install --path punk-run    # orchestration CLI (scaffold)
```

## punk (verification)

```sh
punk init       # scan project, detect conventions
punk plan       # generate implementation contract
punk check      # verify diff against contract
punk receipt    # cryptographic completion proof
punk status     # current workspace state
punk config     # manage configuration
```

## punk-run (orchestration, in progress)

```sh
punk-run status   # show tasks, slots, receipts (not yet implemented)
punk-run config   # show loaded configuration (not yet implemented)
```

See [ROADMAP-v2.md](../docs/product/ROADMAP-v2.md) for implementation plan.
