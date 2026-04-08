# punk (legacy nested workspace)

This directory is the older nested Rust workspace kept as source material while `specpunk` converges on the current root-level `punk` CLI.

If you want the real operator-facing command surface, treat these as the canonical docs:

- `../README.md`
- `../docs/product/CLI.md`
- `../docs/product/ARCHITECTURE.md`

## What lives here

Cargo workspace with 4 legacy crates:

| Crate | Type | Role |
|-------|------|------|
| `punk-core` | library | older implementation source material |
| `punk-cli` | binary | legacy verification-oriented CLI |
| `punk-orch` | library | legacy orchestration source material |
| `punk-run` | binary | legacy orchestration/admin CLI |

## Canonical CLI today

Install the current root CLI from the repo root:

```sh
cargo install --path crates/punk-cli
```

Primary commands:

```sh
punk init --enable-jj --verify
punk init --project <id> --enable-jj --verify

punk go --fallback-staged "<goal>"

punk start "<goal>"
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>

punk status [id]
punk inspect project
punk inspect work [id]
punk inspect <id> --json
punk vcs status
punk vcs enable-jj
```

The default happy path is:

```sh
punk go --fallback-staged "<goal>"
```

The staged/manual path is:

```sh
punk start "<goal>"
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>
```

## Legacy nested binaries

If you intentionally work inside this nested workspace itself:

```sh
cargo install --path punk-cli
cargo install --path punk-run
```

Those nested binaries still expose the older `plan/check/receipt/config`-style surfaces. Treat them as historical implementation material, not as the current product shell described above.

## More context

- `../README.md` — current project overview and operator flow
- `../docs/product/CLI.md` — current command semantics
- `../docs/product/NORTH-ROADMAP.md` — strategic backlog
- `../docs/product/ROADMAP-v2.md` — implementation plan
