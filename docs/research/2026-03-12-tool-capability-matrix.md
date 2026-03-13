---
title: "Specpunk Tool Capability Matrix"
date: 2026-03-12
status: memo
origin: follow-up after research gap mapping
scope: local corpus only
---

# Specpunk Tool Capability Matrix

## Verdict

Specpunk can be honestly described as **tool-agnostic only at the repo-native control layer**.

The `reasoning-aware` layer is **not** tool-agnostic today.

The best product stance is:

- `portable core` across coding tools that can read/write a repo
- `tool-specific enhancers` where transcript surfaces, hooks, and reasoning access are materially better

That leads to a straightforward support posture:

- `Tier A`: Claude Code
- `Tier B`: Cursor, Codex CLI
- `Tier C`: Gemini CLI

## Scope Note

This memo is grounded only in the current local corpus.

Important:

- `Strong` means the corpus supports promising this capability.
- `Degraded` means a weaker or indirect path exists.
- `Unverified` means the current corpus does not justify promising it.
- `No` means the corpus directly supports that the capability is unavailable or not accessible in the needed form.

## What Is Actually Portable

These features do **not** need privileged access to model reasoning:

- repo-native artifacts such as `intent.md`, `glossary.md`, `invariants.md`, `scope.yml`, `evidence.md`
- code-only bootstrap from files and module structure
- scope enforcement from changed files / `git diff`
- terminology and invariant checks over code + artifacts
- review bundle generation in CLI/CI
- standing agent context via repo files such as `AGENTS.md`, `CLAUDE.md`, and rule folders

Why this is credible:

- GitHub Spec Kit is explicitly positioned as a repo/folder-based approach that supports many agent platforms, including `Claude Code` and `Gemini CLI` (`2026-03-12-spec-driven-development.md:75-77`)
- `AGENTS.md` is already treated in the corpus as a tool-agnostic convention gaining traction (`2026-03-12-spec-driven-development.md:99-107`)
- the current product thesis already frames Specpunk as `repo-native`, `CLI-first`, and `CI/PR` integrated, with tool-agnostic support as a goal rather than an editor replacement (`2026-03-12-next-step-product-thesis.md:139-145`)

## Where Portability Breaks

Portability breaks when the product depends on:

- raw reasoning access
- real-time transcript interception
- hook-driven pre-compaction capture
- dynamic in-session intent retrieval
- reliable sub-agent attribution from vendor-native traces

Those surfaces vary a lot by tool.

## Substrate Matrix

| Surface | Claude Code | Codex CLI | Cursor | Gemini CLI | Evidence |
|---|---|---|---|---|---|
| Session file access | `Strong` | `Strong` | `Degraded` | `Strong` | Session files are locally present for all four, but Cursor requires SQLite access and undocumented schema (`2026-03-12-delve-thinking-blocks.md:944-1009`) |
| Reasoning access | `Strong` | `No` | `Degraded` | `No` | Claude stores plaintext thinking; Codex reasoning is encrypted; Gemini stores only final visible responses; Cursor has partial community-observed `thinking` fields (`2026-03-12-delve-thinking-blocks.md:944-1030`) |
| Historical session import | `Strong` | `Degraded` | `Degraded` | `Degraded` | Claude native import is straightforward; SpecStory offers a portable conversation-history path for Claude/Cursor/Codex, but not reasoning-level extraction (`2026-03-12-intent-preservation.md:165-173`) |
| Real-time capture hooks | `Strong` | `Unverified` | `Unverified` | `Unverified` | Claude has documented `PreCompact` and `Stop` hooks with live `transcript_path` access (`2026-03-12-delve-thinking-blocks.md:590-607`, `2026-03-12-delve-thinking-blocks.md:1144-1149`) |
| Dynamic in-session retrieval | `Strong` | `Unverified` | `Degraded` | `Unverified` | Claude has a workable pattern through `CLAUDE.md` injection plus optional MCP querying; Cursor has static rule files but the corpus does not show an equivalent dynamic retrieval path (`2026-03-12-delve-thinking-blocks.md:1159-1168`, `2026-03-12-spec-driven-development.md:99-107`) |
| Sub-agent visibility | `Degraded` | `Unverified` | `Degraded` | `Unverified` | Claude exposes `isSidechain` blocks in the parent session; Cursor stores `agent-transcripts/`; the current corpus does not establish equivalent support for Codex or Gemini (`2026-03-12-delve-thinking-blocks.md:748-753`, `2026-03-12-delve-thinking-blocks.md:1003-1012`) |
| Retention clarity | `Strong` | `Strong` | `Unverified` | `Strong` | Claude and Codex are described as indefinite; Gemini as 30 days by default and configurable; Cursor retention is not clearly established in the corpus (`2026-03-12-delve-thinking-blocks.md:944-1000`) |

## Product Feature Matrix

This is the matrix that matters for Specpunk itself.

| Specpunk feature | Claude Code | Codex CLI | Cursor | Gemini CLI | Product note |
|---|---|---|---|---|---|
| Repo-native intent pack | `Strong` | `Strong` | `Strong` | `Strong` | This is the portable core |
| Code-based draft generation | `Strong` | `Strong` | `Strong` | `Strong` | External analysis, independent of reasoning access |
| Scope enforcement | `Strong` | `Strong` | `Strong` | `Strong` | External CLI/CI check over actual diff |
| Review bundle generation | `Strong` | `Strong` | `Strong` | `Strong` | External CLI/CI artifact generation |
| Session-aware draft generation | `Strong` | `Degraded` | `Degraded` | `Weak` | Depends on transcript richness and accessibility |
| Reasoning-aware decision extraction | `Strong` | `No` | `Degraded` | `No` | This is where `Claude-first` becomes real |
| Live intent retrieval during execution | `Strong` | `Unverified` | `Degraded` | `Unverified` | Requires either hook/MCP surface or a documented substitute |
| Sub-agent attribution in review | `Degraded` | `Unverified` | `Degraded` | `Unverified` | Possible, but not portable enough for a universal promise |

## Recommended Support Tiers

### Tier A: Claude Code First-Class

What should be promised:

- full portable core
- native session import
- reasoning-aware draft extraction
- real-time capture via hooks
- dynamic retrieval patterns via injected context and MCP-style integration

Why:

- plaintext JSONL
- accessible thinking blocks
- documented hook surface
- visible sub-agent traces

### Tier B: Cursor and Codex CLI Compatible

What should be promised:

- full portable core
- conversation-aware import where possible
- degraded session-derived drafting

What should **not** be promised:

- reliable reasoning extraction parity with Claude
- real-time hook-based capture parity
- uniform sub-agent visibility

Why:

- Codex reasoning is encrypted and therefore unusable for product features that require raw rationale
- Cursor is more accessible than Codex on some fronts, but its local storage is schema-fragile and harder to operationalize

### Tier C: Gemini CLI Baseline

What should be promised:

- full portable core
- code-based drafts
- post-hoc review/evidence flow

What should **not** be promised:

- reasoning-aware extraction
- rich session-derived intent
- parity with Claude-first enhancements

Why:

- the corpus shows final-response storage but no separate reasoning blocks

## Product Consequences

### 1. The thesis should stay portable, but the messaging must be asymmetrical

Good phrasing:

- `repo-native intent-control and verification layer`
- `portable core with richer adapters on some tools`

Bad phrasing:

- `same session-aware experience across all coding agents`
- `uniform reasoning-aware layer across Claude Code, Codex, Cursor, and Gemini`

### 2. V1 should not depend on reasoning extraction

V1 should stand on:

- intent pack files
- scope checks
- terminology/invariant checks
- evidence/review bundle generation

Session-derived drafting should be an accelerator, not the foundation.

### 3. Claude-specific enhancements should be explicit, not hidden

The corpus supports a strong `Claude-first` enhancement story. That is a feature, not an embarrassment, as long as the product does not pretend that every tool gets the same experience.

### 4. Benchmark design should separate portable value from enhanced value

The benchmark should answer two different questions:

1. Does the portable core help regardless of tool?
2. How much extra value comes from richer session surfaces on Claude Code?

The current benchmark protocol already points in this direction with an explicit cross-tool phase (`2026-03-12-brownfield-benchmark-protocol.md:460-463`).

## Build Order Implication

The highest-leverage implementation order is:

1. portable core
2. Claude Code adapter
3. Cursor adapter
4. Codex adapter
5. Gemini baseline adapter

Not the other way around.

## Open Questions

These remain unresolved in the current corpus:

- Is there a documented Codex integration surface that can substitute for Claude-style hooks?
- How stable is Cursor schema evolution across versions for a production extractor?
- Is a `SpecStory`-style normalized conversation import enough for non-Claude tools, or does the product need native adapters?
- Should `tool-agnostic` in public messaging mean `same core artifacts` or `same end-to-end workflow`?

## Bottom Line

Specpunk should not frame itself as a uniformly tool-agnostic reasoning layer.

It should frame itself as:

- a **tool-agnostic repo control layer**
- with **Claude-first reasoning-aware enhancements**
- and **degraded but still useful compatibility** on Codex, Cursor, and Gemini CLI

That is both more accurate and more defensible against the current research base.
