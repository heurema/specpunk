# punk CLI

This document defines the target command surface for the first working version.

Unless explicitly marked as planned, commands in this document refer to the current v0/v0.1 CLI surface.

---

## 1. Two surfaces

### Interactive shell

Planned for **Stage 1**. Not implemented in the current baseline.

```bash
punk
```

### Non-interactive CLI

```bash
punk <command> ...
```

Both must route into the same application services.

---

## 2. Canonical modes

`punk` uses three runtime modes:

- `plot`
- `cut`
- `gate`

Prompt examples:

```text
[PLOT repo] >
[CUT repo run_01] >
[GATE repo run_01] >
```

---

## 3. Primary entrypoints

These are the preferred user-facing commands in the current v0/v0.1 surface.

### One-face operator shell contract

For initialized repos, the shell should behave as one obvious top-level interface:

- plain user goal in
- one concise progress or blocker summary out
- one obvious next step out

That means:

- the default happy path is goal-first and shell-first
- `plot`, `cut`, and `gate` remain available, but are not the first thing a normal operator should need to think about
- blocked autonomy should degrade to one explicit recovery path, not to pipeline archaeology
- if the inner loop needs another opinion, the default path should be another model/provider or a bounded council step, not a user interruption
- the user should only re-enter at a true terminal blocker or at the final result/review boundary

### Primitive vs mechanism rule

The commands below are not all ontology-level primitives.

- `Project`, `Goal`, `Contract`, `Scope`, `Workspace`, `Run`, `DecisionObject`, `Proofpack`, and `Ledger` are the deeper primitives.
- shell commands such as `init`, `start`, `go`, `status`, and `inspect` are mechanisms built over those primitives.
- `plot`, `cut`, and `gate` are best understood as permission boundaries over primitive operations.

### Command-to-primitive map

| Command/mechanism | Type | Primary primitives touched |
|---|---|---|
| `punk init` | shell bootstrap | `Project`, `Ledger` |
| `punk start` | staged shell intake | `Goal`, `Contract`, `Scope` |
| `punk go --fallback-staged` | autonomous shell intake | `Goal`, `Contract`, `Run`, `DecisionObject`, `Proofpack`, `Ledger` |
| `punk plot ...` | substrate permission boundary | `Contract`, `Scope` |
| `punk cut run ...` | substrate permission boundary | `Workspace`, `Run`, `Receipt` |
| `punk gate ...` | substrate permission boundary | `DecisionObject`, `Proofpack` |
| `punk status` | shell read surface | `Ledger` projections |
| `punk inspect` | shell read surface | canonical artifacts + `Ledger` projections |

### Project bootstrap

```bash
punk init --enable-jj --verify
punk init --project <id> --enable-jj --verify
```

Use `--project <id>` only when the repo basename is not a suitable project id.

Bootstrap should:

- pin the current repo as a known project
- create repo-local agent guidance
- verify status scope and VCS mode
- converge project-local intelligence toward one inspectable project packet over time

Generated guidance:

- `AGENTS.md`
- `.punk/AGENT_START.md`

Current project inspect surface:

- `punk inspect project`
- `punk inspect project --json`

Current `inspect project` overlay should include:

- existing project capability summary (`bootstrap_ready`, `project_guidance_ready`, `staged_ready`, `autonomous_ready`, `jj_ready`, `proof_ready`)
- derived `harness_summary` fields from current repo state only:
  - `inspect_ready`
  - `bootable_per_workspace`
  - `ui_legible`
  - `logs_legible`
  - `metrics_legible`
  - `traces_legible`
- derived persisted harness packet fields:
  - `harness_spec_ref`
  - `harness_spec`

Current persisted harness packet behavior:

- `punk inspect project` writes a derived repo-local packet to `.punk/project/harness.json`
- `punk inspect project --json` includes both `harness_summary` and the persisted `harness_spec` payload
- the current packet is still inspect-only and derived from repo markers
- derived `validation_recipes[]` now include repo-local `artifact_assertion` entries for persisted bootstrap/guidance refs when a default profile is emitted
- this slice does not add new runtime execution semantics beyond the already-supported `artifact_assertion` recipe

Current work-ledger inspect surface:

- `punk inspect work`
- `punk inspect work <id>`
- `punk inspect work <id> --json`

Current proof inspect surface:

- `punk inspect proof_<id> --json`
- `punk inspect proof_<id>`

Current human-facing inspect expectations:

- `punk inspect work` should show a concise latest-proof evidence summary when a latest proof exists
- `punk inspect work` may also show a concise latest-proof harness summary derived from the latest proof's declared and executed harness evidence
- declared harness evidence in human summaries should preserve any declared `source_ref` when the proof carries it
- `punk inspect proof_<id>` should render a concise human summary for typed `command` evidence, `declared_harness_evidence`, and executed `harness_evidence` without requiring raw JSON reading
- JSON object inspect remains the source of full structured proof detail

Current JSON artifact expectations:

- `DecisionObject` and `Proofpack` keep `command_evidence` as the executed command-check record
- they may also carry additive `declared_harness_evidence` copied from `.punk/project/harness.json`
- `declared_harness_evidence` is metadata only for non-command harness surfaces declared by the persisted packet
- they may also carry additive `harness_evidence` for executed non-command harness recipes
- the first supported executed non-command recipe is `artifact_assertion` from profile-local `validation_recipes[]`
- `artifact_assertion` verifies repo-relative artifact existence and can block `gate` on failure
- current slices still do **not** imply runtime execution of UI, log, metric, or trace recipes

Current status behavior:

- `punk status` now prefers `WorkLedgerView` for current work continuity fields
- recovery-oriented status should surface durable autonomy-linked fields such as `autonomy_outcome`, `recovery_contract_ref`, and a shell-level `suggested_command`
- latest work/lifecycle/next-action data should come from the derived work view, not direct raw-event scanning
- when choosing the implicit current work item, `punk status` / `punk inspect work` should prefer the feature with the latest ledger activity (`run` / `receipt` / `decision` / `proof` / autonomy record), not merely the most recently drafted feature timestamp
- stale/orphaned `running` runs with a dead executor pid, no receipt/decision/proof, and old `heartbeat.last_progress_at` should be ignored by active status/work projections instead of dominating current-work selection
- non-destructive hygiene should start with `punk gc stale --dry-run`, which reports `safe_to_archive` vs `manual_review` candidates without deleting or moving artifacts yet
- normal shell flows may autonomously quarantine `safe_to_archive` stale runs into `.punk/archive/runs/<run-id>/` so the user does not have to participate in inner-loop ledger hygiene
- autonomous stale-run quarantine must remain non-destructive: archive/move is allowed, hard delete is not
- ambiguity that only requires another technical opinion should stay inside the loop and use internal model/provider escalation rather than surfacing as a user question

Longer-term shell expectation:

- project bootstrap and shell status should keep converging on this same inspectable project-intelligence packet
- work continuity should keep converging on one inspectable work-ledger view instead of ad hoc latest-artifact inference

### Default autonomous intake

```bash
punk go --fallback-staged "<goal>"
```

Behavior:

- accepts a plain goal, not a user-written task decomposition
- drafts and approves a contract internally
- runs `cut`
- runs `gate`
- writes proof artifacts
- exits non-zero when verification blocks or escalates
- prepares a staged recovery contract when `--fallback-staged` is enabled

Default shell expectation:

- this is the normal initialized-repo path
- the operator should receive one concise shell summary
- lower-level commands should only be needed for debugging, review, or explicit control

### Staged/manual intake

```bash
punk start "<goal>"
```

Behavior:

- accepts a plain goal
- drafts a contract
- prints the next explicit operator step

Use this when:

- autonomy is blocked
- exact human review is needed between stages
- an expert operator wants tighter manual control

### Mode-level commands

```bash
punk plot contract "<prompt>"
punk plot refine <contract-id> "<guidance>"
punk plot approve <contract-id>
punk cut run <contract-id>
punk gate run <run-id>
punk gate proof <run-id|decision-id>
punk gc stale --dry-run
punk status [id]
punk inspect <id> --json
```

---

## 4. Interactive shell commands

The shell surface below is **planned**, not implemented yet.

Mode switches:

```text
:plot
:cut
:gate
```

Context:

```text
:use <id>
:status
:q
```

Plain text behavior:

- in `plot`: draft/refine contract for active feature
- in `cut`: execute against active approved contract
- in `gate`: review and decide active run

---

## 5. Minimal command semantics

### `punk init [--project <id>] --enable-jj --verify`

Bootstraps the current repo for `punk`.

Creates or refreshes:

- project pin / resolver entry
- bootstrap guidance
- repo-local agent instructions
- default `.gitignore` coverage for `.punk/`, `target/`, and `.playwright-mcp/`
- successful `cut run` receipts should also backfill the same safe `.gitignore` coverage without surfacing `.gitignore` itself as bounded user work

Verifies:

- resolved project id
- status scope
- VCS mode

### `punk go [--fallback-staged] "<goal>"`

Runs the autonomous path from a plain goal.

Internally this executes:

- `plot contract`
- internal approval
- `cut run`
- `gate run`
- `gate proof`

Returns:

- concise human summary with outcome / basis / proof
- structured JSON when `--json` is requested
- non-zero exit for `block` or `escalate`

Auto-chain expectation:

- if the first accepted cycle only proves a controller-created bootstrap scaffold
- and the original goal still clearly asks for implementation work (`implement`, `add`, `support`, `wire`, or `with tests`)
- `punk go` should immediately run one bounded follow-up cycle instead of stopping at the bootstrap proof
- for greenfield Rust bootstrap+implementation goals, that follow-up cycle should narrow toward implementation files like `crates/<app>-cli/src/main.rs`, `crates/<app>-core/src/lib.rs`, and `tests`, rather than reusing the original broad bootstrap prompt unchanged
- if that narrowed follow-up goal declares an exact touch set, later proposal repair must not re-expand it back to bootstrap scope (`Cargo.toml`, `crates`, etc.)
- timeout bootstrap fallback must only preserve scaffold scope for prompts that still actually request scaffold/bootstrap work; rich follow-up implementation prompts on an already bootstrapped repo should stay file-bounded
- timeout fallback must also preserve explicit implementation semantics from the prompt (`--json`, `--force`, `--project-root`, named starter files) instead of collapsing them into a generic bounded slice
- the final shell summary / JSON payload should report the follow-up cycle as the main result and keep the bootstrap cycle as auxiliary context

When `--fallback-staged` is set and autonomy blocks:

- a staged recovery contract is drafted automatically
- recovery metadata and next command are returned
- an autonomy-linked durable record is written so later inspection does not depend on old shell output

The intended durable behavior is stronger than shell text:

- blocked or escalated autonomy should remain inspectable later
- the shell summary should point to durable refs
- later `status` / `inspect work` should be able to answer what happened and what comes next without relying on old terminal output

Preflight expectation:

- if no Git or jj repo is detected, `punk go` should first auto-run `git init` in place and continue in degraded git-only mode instead of failing before intake
- if that automatic `git init` fails, return one explicit recovery path instead of a downstream adapter or scan error
- recovery should point to `git init`, then `punk init --project <id> --enable-jj --verify`, then retry the original `punk go ...`
- for a bootstrapped greenfield Rust repo with no existing inferred checks yet, a goal that explicitly asks to scaffold Rust (`rust`, `cargo`, `crate`, or `workspace` + `scaffold`/`init`/`bootstrap`) may derive an initial `cargo test` or `cargo test --workspace` intake check instead of failing at repo scan
- for a bootstrapped greenfield Go repo with no existing inferred checks yet, an explicit Go scaffold goal may derive `go test ./...` plus scaffoldable scope around `go.mod`, `cmd`, `internal`, and `pkg`
- for a bootstrapped greenfield Python repo with no existing inferred checks yet, an explicit Python scaffold goal may derive `pytest` plus scaffoldable scope around `pyproject.toml`, `src`, and `tests`
- for a bootstrapped greenfield TypeScript/Node repo with no existing `package.json` or inferred checks yet, an explicit TS/Node scaffold goal may derive `npm test` plus scaffoldable scope around `package.json`, optional `tsconfig.json`, and `src`/`tests` (or workspace-style `packages`/`apps`)
- that same greenfield Rust scaffold intake should prefer scaffoldable Rust/workspace surfaces like `Cargo.toml`, `crates`, and `tests` over existing docs/archive files when synthesizing initial scope candidates
- if the repo root has no explicit integrity story but nested manifests/scripts do, `punk go` may infer trustworthy intake checks from nested `Cargo.toml`, nested `package.json` scripts, or nested `Makefile` `test` targets instead of failing immediately at repo scan

### `punk start "<goal>"`

Runs the staged/manual intake from a plain goal.

Preflight expectation:

- if no Git or jj repo is detected, `punk start` should first auto-run `git init` in place and continue in degraded git-only mode instead of failing before intake
- do not defer missing-VCS handling to a later drafter or repo-scan failure
- if that automatic `git init` fails, return one explicit recovery path: `git init`, then `punk init --project <id> --enable-jj --verify`, then retry `punk start "<goal>"`
- for a bootstrapped greenfield Rust repo with no existing inferred checks yet, an explicit Rust scaffold goal may derive an initial `cargo test` or `cargo test --workspace` intake check instead of failing at repo scan
- for a bootstrapped greenfield Go repo with no existing inferred checks yet, an explicit Go scaffold goal may derive `go test ./...` and scaffoldable Go scope instead of failing at repo scan
- for a bootstrapped greenfield Python repo with no existing inferred checks yet, an explicit Python scaffold goal may derive `pytest` and scaffoldable Python scope instead of failing at repo scan
- for a bootstrapped greenfield TypeScript/Node repo with no existing `package.json`, an explicit TS/Node scaffold goal may derive `npm test` and manifest-first scaffold scope instead of failing at repo scan
- that same greenfield Rust scaffold draft should route scope toward scaffoldable Rust/workspace surfaces instead of existing docs/archive paths
- if an explicit bootstrap prompt names nested scaffold touch targets under a missing scaffold root (for example `crates/pubpunk-cli` before `crates/` exists), the draft should still preserve those repo-relative paths in `allowed_scope` instead of collapsing back to only the root manifest
- when a plain Rust bootstrap goal also mentions implementation semantics like `pubpunk init`, controller scaffold member inference should prefer the goal-derived app slug (`pubpunk-cli` / `pubpunk-core`) instead of article-shaped placeholders like `a-cli` / `a-core`
- if the repo root has no explicit integrity story but nested manifests/scripts do, `punk start` may infer trustworthy intake checks from nested `Cargo.toml`, nested `package.json` scripts, or nested `Makefile` `test` targets instead of failing immediately at repo scan
- when those nested-manifest fallbacks are active and the prompt clearly looks backend/data-oriented (`db`, `session`, `seed`, `service`, `dispatch`, etc.), candidate targeting should bias away from obvious UI/generated surfaces like `.astro`, `astro.config.*`, `dist`, and `packs`, and prefer source-first anchors such as `package.json`, `drizzle.config.*`, `src/lib/db/*`, `src/lib/persistence/*`, and `src/actions/*`

Creates:

- `Feature`
- draft `Contract`

Timeout expectation:

- if the contract drafter times out, `punk start` should attempt one deterministic bounded fallback derived from the repo scan and explicit prompt details before returning an error
- if the drafter times out with no captured draft-JSON progress (including silent timeouts and MCP-only chatter), `punk start` should skip compact retry and fall back immediately instead of spending extra time on another blind attempt
- for a bootstrapped greenfield Rust scaffold goal, timeout fallback should preserve scaffoldable Rust/workspace scope (`Cargo.toml`, `crates`, optional `tests`) instead of collapsing into docs/archive candidates
- bounded fallback repair for a plain Rust bootstrap+implementation goal should also preserve scaffoldable workspace scope (`Cargo.toml`, `crates`, optional `tests`) instead of collapsing to manifest-only/file-only recovery that cannot materialize the requested CLI crates
- that same greenfield scaffold-scope preservation must survive later prompt canonicalization and plain prompt mentions like `tests`, so a natural Rust bootstrap+init goal cannot collapse back to `Cargo.toml` + `tests` after repair
- for bootstrapped greenfield Go and Python scaffold goals, timeout fallback should preserve their manifest-first scaffold scope (`go.mod`/`pyproject.toml` plus ecosystem directories) instead of collapsing into docs/archive candidates
- for a bootstrapped greenfield TypeScript/Node scaffold goal, timeout fallback should preserve manifest-first scaffold scope (`package.json`, optional `tsconfig.json`, plus `src`/`tests` or workspace directories) instead of collapsing into docs/archive candidates
- that same timeout fallback should keep scaffold-oriented `expected_interfaces` / `behavior_requirements` derived from the prompt and manifest kind instead of generic `approve-ready bounded contract` placeholder text
- when timeout fallback keeps or recovers file-level `entry_points`, it must re-add any missing entry points into `allowed_scope` before validation so recovery does not fail on self-inconsistent scope coverage
- the same entry-point coverage repair should also apply to normal draft/refine bounded-repair paths, so intake does not fail solely because a drafter returned a self-inconsistent `entry_points` vs `allowed_scope` shape
- non-timeout drafter failures should still fail closed

Writes:

- `.punk/features/<feature-id>.json`
- `.punk/contracts/<feature-id>/v1.json`
- `feature.created`
- `contract.drafted`

### `punk plot contract "<prompt>"`

Creates:

- `Feature`
- `Contract` with status `draft`

Writes:

- `.punk/contracts/<feature-id>/v1.json`
- `feature.created`
- `contract.drafted`

### `punk plot refine <contract-id> "<guidance>"`

Updates:

- existing `Contract` with status `draft`

Timeout expectation:

- if the refine drafter times out, `punk plot refine` should attempt one deterministic fallback by reusing the current draft plus explicit guidance overrides before returning an error
- non-timeout refine failures should still fail closed

Writes:

- updated `.punk/contracts/<feature-id>/vN.json`
- `contract.refined`

### `punk plot approve <contract-id>`

Updates:

- `Contract.status = approved`

Writes:

- `contract.approved`

### `punk cut run <contract-id>`

Creates:

- `Task`
- `Run`
- `Receipt`

Uses:

- `jj` preferred
- `git` fallback

Writes:

- `task.queued`
- `task.claimed`
- `run.started`
- `receipt.written`
- `run.finished`

Behavior notes:

- if bounded execution makes no additional edits because a file-bounded slice's approved entry points were already changed in-scope before dispatch, `cut run` may still record a successful run summary explaining that the slice was already satisfied before dispatch; directory-scoped implementation contracts must still fail honestly as no-progress
- if bounded execution emits noisy progress lines but still makes no entry-point changes and never reaches meaningful compile/check progress, `cut run` may still fail closed as no-progress instead of waiting indefinitely on output noise
- if file-level entry points like `Cargo.toml`, `go.mod`, `pyproject.toml`, or `package.json` were missing at baseline and remain missing after bounded dispatch, `cut run` should still fail closed as no-progress instead of hanging indefinitely on executor noise
- manifest-only greenfield bootstrap runs should use a short executor timeout budget instead of waiting for the full generic execution timeout before normalizing to blocked/no-progress
- if that same greenfield missing-manifest bootstrap case later degrades into timeout, stall, or orphan classification, `cut run` should still collapse it into the same deterministic blocked summary instead of flapping between blocked and stall-style failures
- for an approved Rust workspace bootstrap contract that explicitly names crate directories under `crates/...`, `cut run` may materialize a minimal controller-owned workspace scaffold inside `allowed_scope` before dispatch so execution starts from concrete files instead of immediately blocking on an empty layout
- for generic Rust workspace bootstrap contracts whose scope is only `Cargo.toml`, `crates`, and `tests`, `cut run` may infer bounded crate members from the contract semantics (for example a named CLI like `pubpunk`) and materialize the corresponding minimal workspace scaffold inside `allowed_scope`
- when cargo-based bounded execution would generate a new `Cargo.lock` outside `allowed_scope`, `cut run` should prune that generated side effect before writing the receipt so the project is not left with avoidable out-of-scope dirt, even for file-scoped follow-up Rust slices
- when bounded execution only performs inspection/check commands inside allowed scope, makes no product-file changes, and then stalls without emitting a sentinel, `cut run` should normalize that case into deterministic no-progress failure instead of surfacing raw noisy stall tails like `mcp: ...` or `succeeded in ...`
- executor prompts should include the original approved goal text alongside condensed behavior requirements so bounded slices do not lose critical implementation details when drafter requirements are abbreviated
- when `allowed_scope` is directory-scoped but bounded, executor prompts should also enumerate the current in-scope files available for direct edit so the model does not stall on ambiguous directory-level scope
- executor may narrow a small directory-scoped bounded contract to the currently existing in-scope files for execution only, which allows fail-closed context packing and patch-apply routing without mutating the persisted approved contract
- for recurring bounded product slices with stable three-file topology (for example a Rust `core + cli + tests` init slice), execution may inject controller-owned recipe/patch seeds so patch/apply starts from concrete edits instead of stalling on the full contract prose
- for the stable bootstrapped `pubpunk init` recipe (`crates/pubpunk-core`, `crates/pubpunk-cli`, `tests`), `cut run` may short-circuit through a controller-owned implementation template before verification instead of waiting on repeated no-progress patch/apply attempts
- that controller-owned `pubpunk init` template should materialize the canonical `.pubpunk` tree (`style/examples`, `targets`, `review`, `lint`, `local/{drafts,reports,cache,generated}`), keep `--json` / `--force` / `--project-root` wiring intact, and write the exact starter `project.toml` shape required by the richer completion contract
- if a bounded follow-up Rust slice allows `tests` but the current repo only has placeholder files like `tests/README.md`, execution may synthesize one concrete in-scope test entry point (for example `tests/init_json.rs`) so patch/apply can create real coverage without widening the approved persisted contract
- whenever a bounded Rust slice narrows a `tests` directory into concrete file paths, execution should drop placeholder-only test files from the narrowed execution scope so the bounded slice stays patch/apply-sized instead of tipping into the noisier general exec lane
- if a bounded patch/apply Rust slice exposes sequential failed checks (for example one compile error in `cargo test -p <crate>` and then a later failure in `cargo test --workspace`), `cut run` may spend one bounded repair pass per newly exposed failed check, capped at three total patch/apply passes
- if a bounded patch/apply slice emits prompt/setup text and then goes silent without producing a patch, `cut run` should treat that as a no-output stall, spend at most one bounded retry on it, and then collapse unchanged entry points back into deterministic no-progress instead of waiting for the full raw timeout repeatedly
- if patch/apply partially mutates multiple files and a later hunk fails or validation detects that a previously non-empty source file became zero-byte, `cut run` must restore the original contents of every touched file and surface a blocked corruption summary instead of leaving the repo in a damaged state
- if a blocked or failed patch/apply attempt damages previously non-empty entry-point files outside the final validated patch (for example by leaving them zero-byte or missing), `cut run` must restore those original entry-point contents before returning the blocked result
- `already satisfied in allowed scope before bounded dispatch` is only valid for deterministic file-bounded no-progress slices; blocked summaries must stay blocked and must not be upgraded to success
- if a bounded implementation run reports `PUNK_EXECUTION_COMPLETE` but the observed repo change set is still empty, `cut run` must normalize that into deterministic no-progress/failure instead of writing a false success receipt
- if `cut run` executes inside an isolated git worktree and produces product-file changes, those in-scope file edits must be synced back into the main repo root before the receipt is written so later `gate` / `proof` phases and the operator-visible worktree see the same result
- if `cut run` executes inside an isolated git worktree while the main repo root already contains uncommitted product files from an earlier stage (for example bootstrap-created manifests or sources), those present repo-root files must be copied into the isolated workspace before execution so follow-up bounded slices see the same baseline instead of starting from bare `HEAD`
- if `cut run` needs an isolated git workspace in degraded git-only mode on a repo that has no committed `HEAD` yet, it should create an unborn isolated branch/worktree instead of failing with `invalid reference: HEAD`
- `gate` and `proof` remain authoritative; this cut-time success does not replace verification

### `punk gate run <run-id>`

Reads:

- approved contract
- receipt

Produces:

- `DecisionObject`

Checks:

- scope
- policy
- target checks
- integrity checks

Behavior notes:

- `gate run` must never accept a run whose receipt status is not `success`, even if trusted target and integrity checks happen to pass afterward
- `gate run` must also block a bounded implementation receipt that claims success while reporting no observable repo changes, unless the receipt explicitly says the slice was already satisfied before bounded dispatch
- controller-owned runtime artifacts written under `.punk/runs/<run-id>/...` should not count as user scope violations during `gate run`; scope validation should judge only repo changes attributable to the bounded work itself
- when a run executed inside an isolated VCS workspace (for example a git worktree in degraded git-only mode), `gate run` must execute trusted target and integrity checks inside that recorded `workspace_ref`, not back on the original repo root
- if `gate run` executes cargo-based trusted checks for a contract whose scope does not include `Cargo.lock`, a newly generated `Cargo.lock` should be pruned after the check rather than left behind as avoidable project litter

Target and integrity checks must be validated and executed as direct trusted runners, not interpolated through `/bin/sh -lc` or other shell-fragment execution.

### `punk gate proof <run-id|decision-id>`

Produces:

- `Proofpack`

Writes:

- `.punk/proofs/<decision-id>/proofpack.json`
- `proofpack.written`

---

## 6. CLI rules

1. `cut` should refuse unapproved contracts.
2. `gate` is the only path that writes final decision artifacts.
3. `status` must reconstruct state from the event log, not from ad-hoc file scanning.
4. All inspectable commands should support structured output.
5. Plain user goals should route through `punk go --fallback-staged "<goal>"` by default.
6. `punk start "<goal>"` remains the staged/manual escape hatch.
7. New command proposals should state whether they introduce a new primitive or only a new derived mechanism; the default expectation is derived mechanism.
8. Shell-facing commands should minimize reading burden by returning one concise progress or blocker summary plus one obvious next step.

---

## 7. Deliberately postponed commands

Not part of the first working version:

- `goal`
- `queue`
- `daemon`
- `morning`
- `ask`
- `panel`
- `quorum`
- `verify`
- `diverge`
- benchmark commands
