# Research: reliable controller-owned patch lane after plain patch text transport

Date: 2026-04-01
Status: exploratory, actionable

## Question

What is the strongest reliable next step for `specpunk` self-hosting on bounded existing-file glue slices, after:

- separating `exec / patch-apply / manual` lanes
- replacing JSON patch transport with plain patch text
- hardening controller-side patch apply and nested Cargo check roots

## Constraints

- External trees under `/Users/vi/contrib/*` are reference-only.
- No scripts or workflows from those trees were executed.
- We want a solution aligned with the `codex-rs` architecture, not another prompt-only tweak.

## Current local evidence

### Confirmed behavior by run

1. `run_20260401165457725`
   - plain patch text lane returned a real patch
   - controller failed at apply with `patch does not apply`
   - conclusion: patch generation transport improved; apply path became the bottleneck

2. `run_20260401175501761`
   - patch applied
   - changed files appeared in:
     - `punk/punk-orch/src/ratchet.rs`
     - `punk/punk-run/src/main.rs`
   - controller-side checks then failed because `cargo test -p punk-run` ran from the outer root instead of nested `punk/`
   - conclusion: apply path worked; nested check-root inference was the next bottleneck

3. `run_20260401180221816`
   - after apply hardening and nested check-root inference
   - run failed with `codex command timed out after 90s: 64,010`
   - however `.punk/runs/run_20260401180221816/stdout.log` begins with a complete-looking diff and contains both target files
   - conclusion: the remaining bottleneck is no longer mutation ownership or controller-side checks; the controller is still waiting for process exit even after a usable patch is already present in the stream

### Additional concrete observation

The timed-out stdout for `run_20260401180221816` starts with:

```text
diff --git a/punk/punk-orch/src/ratchet.rs b/punk/punk-orch/src/ratchet.rs
```

and includes both intended file edits, but the lane still timed out.

This means the current patch lane still conflates two distinct concerns:

1. `patch became available`
2. `codex process exited cleanly`

For this slice class, those are not the same event.

## Reference findings

### Primary reference: `contrib/openai/codex/codex-rs`

Most relevant modules:

- `apply-patch/`
- `exec/`
- `execpolicy/`
- `core/tests/suite/unified_exec.rs`

### What matters architecturally

1. `exec`, `apply-patch`, and `policy` are separate concerns.
2. `apply_patch` is a dedicated patch grammar and application surface, not “raw model edits repo directly”.
3. `unified_exec` tests show that `apply_patch` is intercepted and completed as a separate lifecycle from generic exec-command completion.
4. `apply-patch` parser is intentionally lenient around wrappers and whitespace, and uses context-seeking rather than depending only on brittle line-number headers.

### What matters for our problem

The reference architecture suggests two strong ideas:

1. **Patch transport should have an explicit patch envelope**.
2. **Controller should stop caring about process lifetime once a valid patch artifact has been observed and accepted**.

## Option comparison

### Option A — keep git unified diff lane, but accept patch on timeout / early-stop on complete diff

Shape:
- keep current `diff --git` transport
- if stdout already contains a parseable complete patch, do not fail on timeout
- optionally stop the process as soon as a complete patch is detected in streamed output

Pros:
- smallest change
- directly addresses the observed `patch printed but process timed out` failure

Cons:
- still uses git unified diff, which is fragile in current evidence
- current timed-out diff was not obviously `git apply --check` clean
- likely improves liveness, but not patch correctness enough

Assessment:
- reliability: medium
- implementation cost: low
- reference alignment: medium

### Option B — switch patch lane from git unified diff to `apply_patch` envelope, with stream completion detection

Shape:
- model returns `*** Begin Patch ... *** End Patch` text, or a blocked sentinel
- controller incrementally watches stdout/stderr
- once a full parseable patch envelope appears, controller stops waiting for normal process exit
- controller validates scope and applies through an internal patch parser/apply path

Pros:
- closest to `codex-rs/apply-patch`
- explicit patch boundary markers make stream completion detection reliable
- avoids overloading `git apply` with malformed or partial unified diff text
- easier to distinguish `complete patch artifact` from `process not exited yet`

Cons:
- requires a local parser/apply path for the patch grammar
- larger implementation than a timeout-only fix

Assessment:
- reliability: high
- implementation cost: medium-to-high
- reference alignment: high

### Option C — two-step generator: target/plan first, patch second

Shape:
- first ask for compact target selection / change plan
- then ask for patch

Pros:
- may reduce cognitive load per generation step

Cons:
- still depends on the same patch transport issues unless combined with Option A or B
- adds protocol complexity before fixing the real artifact boundary

Assessment:
- reliability: medium
- implementation cost: medium
- reference alignment: medium

### Option D — controller first-hunk engine

Shape:
- controller computes and applies a tiny first edit itself
- model continues from a non-blank diff state

Pros:
- deterministic bootstrap

Cons:
- controller becomes a mini code-transform engine
- larger product-specific complexity
- still does not solve patch artifact boundary cleanly

Assessment:
- reliability: potentially high later
- implementation cost: high
- reference alignment: medium

## Recommendation

### Best strong solution

Implement a **reference-aligned `apply_patch` lane with stream completion detection**.

That means:

1. Keep lane routing (`Exec`, `PatchApply`, `Manual`).
2. Change patch generation format from git unified diff to `apply_patch`-style patch envelope.
3. Detect a complete patch artifact in the output stream by explicit `*** Begin Patch` / `*** End Patch` boundaries.
4. Once a complete valid patch is observed, stop waiting for normal process exit.
5. Parse and validate patch locally:
   - allowed scope only
   - existing-file-only for this lane
   - no renames / deletes for glue slices
6. Apply patch through a dedicated local patch apply path, not raw `git apply` as the only authority.
7. Run controller-owned checks after apply.

### Why this is stronger than more prompt tuning

Because the observed failures are now about artifact lifecycle, not just wording:

- patch can exist before process exit
- git unified diff is too brittle as the only patch artifact for this lane
- explicit patch boundaries are a better controller contract than “process exited successfully”

## Tactical fallback

If we need an intermediate bounded step before the full `apply_patch` lane:

- teach the current plain patch text lane to treat `complete parseable patch seen in output` as success-worthy even if the process later times out

This is still weaker than the recommended solution, but it directly addresses the evidence from `run_20260401180221816`.

## Confidence

- Execution lanes are the right architectural direction: **high**
- JSON patch transport was the wrong path: **high**
- The next reliable solution is `apply_patch` envelope + stream completion detection: **medium-high**
- A timeout-only workaround on the current unified diff lane is enough as a final solution: **low**

## Proposed next implementation slice

Title:

> Add a controller-owned `apply_patch` transport with stream completion detection for patch lane

Likely scope:

- `crates/punk-adapters/src/lib.rs`
- maybe `crates/punk-adapters/src/context_pack.rs`
- possibly a small new helper module if the patch parser should not live inline

Acceptance idea:

- eligible glue slices use `apply_patch`-format output, not git unified diff
- controller can detect a complete patch before child exit
- controller no longer fails a run just because `codex exec` times out after already emitting a valid patch artifact
- controller applies only validated, in-scope patch hunks
