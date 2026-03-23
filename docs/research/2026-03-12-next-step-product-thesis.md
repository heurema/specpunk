---
title: "Specpunk Product Thesis"
date: 2026-03-12
status: v2
origin: cross-check + deep research synthesis + thesis review
---

# Specpunk Product Thesis

## One-Line

Repo-native intent layer with automatic draft extraction, scope enforcement, and behavior-first review evidence.

## Thesis

Specpunk is **not** another coding agent, AI IDE, or AI code reviewer.
Specpunk is **not** a spec-as-source compiler (that is CodeSpeak/Tessl territory).

Specpunk is a **portable intent-control and verification layer** that sits on top of existing execution tools (Claude Code, Codex, Cursor) and makes brownfield AI-assisted changes:

- easier to constrain,
- easier to review,
- and easier to trust.

We do not own code generation. We do not regenerate code from intent.
We constrain and verify changes made by external agents, and we compare declared intent with actual diff and behavior evidence.

## Problem

Teams already using coding agents in real codebases hit the same four failures repeatedly:

1. **Intent evaporates**
   The prompt contains the real reasoning, but the repo only keeps the code diff. Session forensics suggest a non-trivial high-value subset of thinking blocks contains architectural decisions, rejected alternatives, and discovered constraints that are discarded after every session. Teammates see code, not decisions.

2. **Review does not scale**
   AI adoption increases PRs by 98% and PR size by 154%, but review time grows 91% (Faros AI, 10K+ devs). DORA 2024: AI adoption correlates with -7.2% stability. The bottleneck shifted from writing to reviewing.

3. **Brownfield changes spill across boundaries**
   Agents touch files outside the intended area because the repo lacks machine-checkable edit scope. CMU study: Cursor adoption → +40% code complexity, +30% static analysis warnings across 807 repos.

4. **Verification is too weak**
   Passing tests and plausible code are not enough. One CodeRabbit-linked dataset reports 1.57x more security findings overall in AI-generated code, with XSS specifically 2.74x worse. Test suites miss 21% of non-equivalent refactorings that differential fuzzing catches (arXiv:2602.15761).

## Core Principle

**auto-draft → human-curated compact artifact → enforcement → evidence**

This is the critical design sequence:

1. **Auto-draft**: Bootstrap intent from sessions, issues, PRs, or code. Never start from blank page.
2. **Human curation**: Engineer edits the draft into a minimal, decision-dense artifact. This is the durable system of record.
3. **Enforcement**: Scope checks, terminology consistency, invariant validation against actual changes.
4. **Evidence**: Behavioral proof that the change matches declared intent.

Why this order matters:
- If extraction is the center and repo artifact is secondary → product becomes a "transcript mining utility"
- If manual artifact is the center without extraction → product becomes "another pile of markdown nobody reads"
- The correct answer is **both**: auto-draft removes friction, human curation creates durability

## Target User

Primary:

- Engineering teams already using AI coding tools in existing codebases
- Leads and senior ICs accountable for review quality and production safety
- Teams with repeated pain around context drift, large PRs, and weak confidence in AI changes

Not the target:

- Solo greenfield vibe-coding
- Teams willing to adopt a brand-new language or IDE wholesale
- Teams looking for full autonomy with minimal human review

## Product Wedge

The first useful product is a **module-level intent pack** plus **scope enforcement** plus **behavior evidence**.

### Core Artifact Set

```
.specpunk/
  modules/
    auth/
      intent.md          # purpose, constraints, key workflows, accepted tradeoffs
      glossary.md         # domain terms, canonical meanings, forbidden synonyms
      invariants.md       # rules that must remain true, API/behavior constraints
    payments/
      intent.md
      glossary.md
      invariants.md
  tasks/
    task-001/
      scope.yml           # allowed files/directories for this task
      evidence.md         # filled after execution: behavior delta, test results, gaps
      review.md           # generated: compact review artifact
```

Each artifact type:

- **intent.md** — module purpose, constraints, key workflows, accepted tradeoffs. It should stay materially shorter than the code it describes. Written once, updated on significant changes.

- **glossary.md** — domain terms with canonical meanings. Forbidden synonyms where ambiguity is dangerous. Machine-checkable against diffs.

- **invariants.md** — rules that must remain true. API contracts, data integrity requirements, safety constraints. Checked against changed code.

- **scope.yml** — explicit allowed files/directories/globs for a task. Checked against actual touched files. Works for both human and agent changes, including delegated sub-agent work.

- **evidence.md** — filled after execution. Not just "tests passed" (CI already shows that). Must include: behavior delta (what changed in observable behavior), new/changed assertions, coverage of the intended change, known gaps and uncertainties.

- **review.md** — generated compact artifact combining: task intent, scope adherence, terminology/invariant warnings, behavior evidence. The intended workflow is that the reviewer reads this first, raw diff second.

### Brownfield Position

Specpunk takes an explicit three-source bootstrap position for intent creation:

1. **From code**: analyze existing module, extract purpose, boundaries, key behaviors
2. **From sessions/issues/PRs**: extract decisions, tradeoffs, constraints from recent work history
3. **From human input**: engineer edits the auto-draft into a curated artifact

Then the artifact can become the durable system of record if the team keeps curating it. No magical full takeover. No purely manual writing.

This is deliberately different from:
- CodeSpeak (full code→spec conversion, spec-as-source)
- Tessl (spec-first, agent registry)
- Manual spec-driven development (blank page problem)

### Multi-Agent Scope

Scope enforcement must work not only on the final diff, but on delegated work:

- When an agent spawns sub-agents, scope constraints propagate
- When multiple agents work on the same repo, scope conflicts are detected
- Review artifact shows which agent touched which files and whether scope was respected

This is motivated by the finding that review consumes 59.4% of all tokens in multi-agent systems.

## Product Shape

The preferred shape is:

- repo-native files (committed, versioned, diffable)
- CLI-first workflow
- CI/PR integration (GitHub Actions, pre-commit hooks)
- tool-agnostic as a product goal (starting with strong support for Claude Code and useful support for Codex, Cursor, and Gemini CLI)

The preferred shape is **not**:

- a replacement editor or terminal agent
- a spec-as-source compiler
- a code generation system
- a deterministic regeneration engine

We do not own deterministic regeneration of code from specs. But deterministic enough evaluation still matters for our own extraction, checking, and benchmark reproducibility.

## Differentiation

| Dimension | CodeSpeak | Tessl | Kiro | Specpunk |
|-----------|-----------|-------|------|----------|
| Core idea | Spec compiles to code | Spec-first agent platform | Structured feature workflow | Intent control + verification layer |
| Owns generation | Yes | Yes | Partially | **No** |
| Repo-native | TBD | Partially | Partially | **Yes** |
| Brownfield | Takeover feature | Registry | Design docs | **Bootstrap + enforce** |
| Verification | Test coverage | Agent validation | Property checks | **Behavior evidence** |
| Tool-agnostic | No (own CLI) | No (own platform) | No (AWS IDE) | **Yes** |
| Non-determinism problem | Must solve | Must solve | Partially | **Not our primary codegen problem** |

## MVP (v0.1)

Version 0.1 does only this:

1. Declare module scope (`scope.yml`)
2. Compare changed files against allowed scope
3. Create or edit module intent pack (with auto-draft from code/sessions)
4. Run terminology consistency check against diff + glossary
5. Collect behavior evidence summary (beyond raw test pass/fail)
6. Produce compact review artifact

What v0.1 does **not** do:

- Generate code
- Full invariant checking (comes in v0.2)
- Mutation testing (comes when tooling is mature per language)
- Cross-repo consistency
- Sub-agent scope propagation (comes in v0.3)

## Example Workflow

1. Engineer selects a module and task.
2. `specpunk init auth` bootstraps intent pack:
   - Analyzes `src/auth/` code → draft `intent.md`
   - Extracts terms → draft `glossary.md`
   - Identifies constraints → draft `invariants.md`
   - Engineer reviews and edits drafts (starting target: single-digit minutes when the module is reasonably bounded)
3. `specpunk scope task-001 --allow "src/auth/**" --allow "tests/auth/**"` declares scope.
4. Engineer uses Claude Code / Codex / Cursor to make the change.
5. `specpunk check`:
   - Files touched outside scope → high-severity warning
   - New terms not in glossary → soft warning
   - Invariant conflicts → high-severity warning
   - Missing behavior evidence → prompt to fill
6. `specpunk review` generates compact review artifact.
7. Reviewer sees:
   - Intent delta (what was meant to change)
   - Scope delta (what actually changed, with violations highlighted)
   - Behavior evidence (not just "tests passed" but "what behavior changed")
   - Raw diff only as supporting detail

## Success Metrics

Primary:

- Reduction in out-of-scope file edits (most measurable, hardest signal)
- Reduction in reviewer uncertainty (measured via confidence survey)
- Higher acceptance confidence for AI-generated changes
- Reduction in review time per AI-assisted change

Secondary:

- Fewer follow-up fixes after merge
- Fewer "what did we decide?" clarifications
- Better consistency of terminology across modules
- Lower ratio of false positive warnings to true positives

## Non-Goals

Specpunk v1 explicitly avoids:

- Building a new IDE
- Replacing terminal coding agents
- Full code generation from natural-language specs
- Whole-repo brownfield takeover (bootstrap one module at a time)
- Formal verification claims
- Auto-updating giant spec trees
- Owning deterministic regeneration of generated code
- Vendor-specific foundation (intent pack is repo-native, not tied to Claude/GPT/Gemini internals)

## Risks

Main product risks:

1. **The intent pack becomes another pile of markdown nobody reads**
   Mitigation: auto-draft extraction + the design rule below + enforcement that references the artifacts.

2. **Scope checks become noisy and get ignored**
   Mitigation: start with hard scope (explicit allow-list), not soft scope (heuristic boundaries). Initial target: keep false positive rate below 10%.

3. **Terminology checks produce too many false positives**
   Mitigation: start with exact-match glossary violations only. Embedding-based similarity comes later, after precision is proven.

4. **Evidence collection is shallow and creates false confidence**
   Mitigation: evidence must include behavior delta, not just test counts. If behavior delta cannot be computed, evidence.md must explicitly state "behavior change not verified."

5. **Tool-agnostic support becomes lowest-common-denominator**
   Mitigation: first-class integration with Claude Code (hooks, MCP), good integration with others. Not everything needs to be equally supported.

6. **Auto-draft extraction becomes the product instead of the artifact**
   Mitigation: extraction is an onboarding feature, not the foundation. The durable system of record is always the repo-native intent pack, not the session log. If extraction breaks, the product still works.

The design rule remains:

**If the artifact is not shorter and more decision-dense than the code it explains, it is a failure.**

## Build Sequence

1. **Benchmark automation skeleton**
   Without a measurement loop, too easy to build features that don't change outcomes.

2. **Scope enforcement CLI**
   Simplest check, hardest signal: did the agent stay inside the declared boundary?
   - Easy to measure (binary: in scope / out of scope)
   - Minimal false positives (explicit allow-list, not heuristic)
   - Immediate value for any team with boundary discipline
   - Completely tool-agnostic

3. **Automatic intent draft extraction**
   Bootstrap intent pack from code, sessions, issues, PRs.
   - From code: module purpose, key behaviors, boundaries
   - From sessions: decisions, tradeoffs, constraints (thinking blocks where available)
   - From issues/PRs: acceptance criteria, context
   - Output: draft intent.md + glossary.md for human curation

4. **Terminology checker**
   Second fast check after scope:
   - Exact-match glossary violations in diffs
   - Synonym detection (embedding-based, gated behind precision threshold)
   - Cross-module term consistency

5. **Behavior evidence**
   Beyond "tests passed":
   - What behavior changed (input/output delta on affected functions)
   - Property-based testing where applicable
   - Behavior summary in structured format
   - Known gaps explicitly stated

6. **Mutation score** (where viable)
   Language-dependent, heavy, but gold standard for test quality.
   - Not mandatory for v1
   - Add where tooling is mature (Python/Stryker, JS/Stryker)
   - Use as benchmark metric before using as product feature

## Research Base

This thesis is grounded in 22 research documents (13,032 lines, 200+ sources):

| Finding | Source | Implication |
|---------|--------|-------------|
| Intent evaporation is central | All 6 research threads | Core problem to solve |
| Review is the bottleneck, not coding | DORA 2024, Faros AI, METR RCT | Don't accelerate coding, improve review |
| AI code shows materially worse security findings; XSS is 2.74x worse in one CodeRabbit-linked dataset | CodeRabbit-linked dataset, 470 PRs | Verification is non-optional |
| 49-64% user goals missed in code→spec | UCRBench | Full takeover not feasible, bootstrap + curate |
| Terminology drift causes real bugs | Breslav podcast, DDD research | Glossary enforcement is tractable |
| Thinking blocks contain a meaningful high-value subset in sampled session analysis | Session forensics | Auto-draft extraction is viable |
| Scott Logic's headline 10x slowdown overstates the measured ratio, but spec-heavy workflow was still materially slower | Scott Logic reconstruction | Minimal specs, not verbose specs |
| SDD value accrues at team-months, not per-task | Tessl data, historical precedent | Benchmark must measure team outcomes |
| 21% non-equivalences escape test suites | Differential fuzzing study | Tests alone insufficient, behavior delta needed |
| Multi-agent review = 59.4% of tokens | Multi-agent systems research | Scope enforcement for agents, not just humans |
