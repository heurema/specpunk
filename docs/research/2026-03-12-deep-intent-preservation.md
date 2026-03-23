---
title: Intent Preservation in AI-Assisted Programming — Deep Implementation Pass
date: 2026-03-12
source: web-research
type: deep-research-pass-2
tags: [intent-preservation, implementation, schemas, hooks, git-notes, NLP-pipeline, regulated-industries, MLOps]
---

# Intent Preservation — Deep Implementation Pass

> Second-pass research. Assumes first-pass (`2026-03-12-intent-preservation.md`) is read. Does NOT repeat what is already known. Focuses on: exact storage formats, implementation-level details, academic research missed, adjacent industry patterns, concrete NLP pipelines, quantitative data.

---

## 1. Implementation-Level Analysis of Existing Tools

### 1.1 SpecStory — Exact Storage Architecture

**What it captures.** SpecStory CLI wraps terminal agents (Claude Code, Gemini CLI, Codex CLI, Cursor CLI) and auto-saves every session to searchable Markdown. For Claude Code specifically, it reads JSONL session files from `~/.claude/projects/<project-hash>/`, converts them to Markdown, and writes to `.specstory/history/` in the project root.

**Folder structure.**
```
.specstory/
  history/
    2026-03-12_14-22-35_implement-auth-flow.md    # one file per session
    2026-03-11_09-10-01_refactor-database.md
  ai_rules_backups/                               # previous derived-rules versions
```
The `ai_rules_backups/` directory is gitignored automatically. History files are committed if desired.

**Markdown session format.** Each file contains:
- Human prompt as a `##` heading followed by the text
- Assistant response with code blocks preserved
- Tool call output embedded inline
- Diff blocks from Write/Edit operations
- Timestamp metadata in YAML frontmatter

**Cloud API.** SpecStory Cloud exposes:
- REST: list/update/delete projects; list sessions; get recent sessions; check session existence
- GraphQL endpoint for advanced querying with full-text search
- Authentication via cookie-based tokens

**Derived rules.** With `derivedRules: true`, SpecStory extracts behavioral patterns from sessions and writes:
- Cursor: `.cursor/rules/derived-rules.mdc`
- Copilot: `.github/copilot-instructions.md`

**Source / license.** SpecStory CLI is Apache-2.0 Go (~99.8% Go). The skills/agent component is separately open-sourced.

**What is NOT captured.** SpecStory captures conversation text but does not: (a) link sessions to specific commits, (b) parse out individual decisions vs. background discussion, (c) weight or rank decisions by significance, (d) store structured decision records — only raw Markdown. The search is full-text, not semantic.

Sources: [SpecStory CLI](https://specstory.com/specstory-cli) · [getspecstory GitHub](https://github.com/specstoryai/getspecstory) · [SpecStory Features](https://docs.specstory.com/specstory/features)

---

### 1.2 Git AI — Exact Data Structures

**Architecture.** Git AI is a transparent Git wrapper: the `git-ai` binary intercepts `git commit`, `git rebase`, etc. by detecting its own invocation name. It does NOT modify commit SHAs. All attribution data lives in `refs/notes/ai` (separate from the default `refs/notes/commits` namespace — by spec, to avoid tool conflicts).

**Multi-tier storage.**

| Tier | Location | Contents | Lifecycle |
|------|----------|----------|-----------|
| Working logs | `~/.git-ai/repos/<sha>/working_logs/` | Uncommitted checkpoint records, file snapshots | Until committed |
| Git notes | `refs/notes/ai` | Authorship log per commit — permanent | Persists through git operations |
| SQLite | `~/.git-ai/internal/git-ai.db` | AI prompts, transcripts, telemetry | Local only |
| CAS (optional) | Cloud | Deduplicated prompt messages | Cloud-synced |

**Git AI Standard v3.0.0 — Authorship Log format.**

Each note attached to a commit SHA contains two sections separated by `---`:

```
# Authorship Log v3.0.0

## Attestation Section
<line-range attribution data: file, start_line, end_line, author_type>

---

{
  "schema_version": "authorship/3.0.0",
  "git_ai_version": "1.x.x",
  "base_commit_sha": "abc123...",
  "prompts": {
    "<prompt_id>": {
      "agent_id": {
        "tool": "claude-code",
        "model": "claude-sonnet-4-6"
      },
      "human_author": "user@example.com",
      "messages": [
        { "role": "user", "content": "..." },
        { "role": "assistant", "content": "..." }
      ],
      "line_stats": {
        "total_additions": 47,
        "total_deletions": 12,
        "accepted_lines": 43,
        "overridden_lines": 4
      },
      "messages_url": "https://..."
    }
  }
}
```

**Hook system.** Two hooks per supported agent:
- **Pre-edit checkpoint**: captures state before agent touches files; marks human-authored changes since last checkpoint
- **Post-edit checkpoint**: snapshots newly inserted AI code; marks it as AI-authored; consolidates into `AuthorshipLog`

**Rebase re-attribution algorithm.**
1. Snapshot AI attributions from original branch head
2. Walk every new commit produced by rebase in order
3. For each commit: replay its diff via `imara-diff`; find attribution segments that appeared in that diff; write them into the new `AuthorshipLog`

**Supported agents.** Cursor, Claude Code, GitHub Copilot, Gemini CLI, Continue CLI, JetBrains IDEs — each via a standardized `AgentCheckpointPreset` trait that parses agent-specific hook input (JSON, JSONL, SQLite depending on agent).

**What this stores vs. intent.** Git AI captures *who wrote each line* (human or which AI agent/model) and the *prompt that generated it*, but NOT: (a) the reasoning behind the prompt, (b) why alternatives were rejected, (c) design constraints that shaped the decision. The `messages` array in the metadata section contains the raw conversation — extracting structured decisions from it requires a separate step.

Sources: [git-ai GitHub](https://github.com/git-ai-project/git-ai) · [Git AI Standard v3.0.0](https://github.com/git-ai-project/git-ai/blob/main/specs/git_ai_standard_v3.0.0.md) · [DeepWiki analysis](https://deepwiki.com/git-ai-project/git-ai) · [GitHub browser plugin](https://blog.rbby.dev/posts/github-ai-contribution-blame-for-pull-requests/)

---

### 1.3 Archgate — MCP Server and Rules Engine

**Core model.** Archgate stores Architectural Decision Records as plain Markdown files with YAML frontmatter. The MCP server makes these available to agents before they write code.

**ADR frontmatter schema.**
```yaml
---
id: ARCH-003
title: API Route Conventions
domain: backend
rules: true
files:
  - "src/api/**/*.ts"
---
```

Fields: `id` (unique, string), `title`, `domain` (string tag), `rules` (boolean — enables automated checking), `files` (glob array — scopes rule to file patterns).

**Rules engine.** Companion `.rules.ts` files contain TypeScript:
```typescript
import { defineRules } from "archgate/rules";

export default defineRules((ctx) => [
  {
    name: "require-createRoute",
    severity: "error",
    async run() {
      const files = await ctx.glob("src/api/**/*.ts");
      for (const file of files) {
        const content = await ctx.grep(file, "createRoute");
        if (!content.found) {
          return { violated: true, file, line: 1 };
        }
      }
    }
  }
]);
```
Context exposes `glob` and `grep` utilities. Violations report `{ violated, file, line }`.

**Claude Code plugin — five roles.**
1. `developer` — reads ADRs before coding, validates after
2. `architect` — proposes new ADRs when patterns emerge
3. `quality-manager` — runs rule checks on modified files
4. `adr-author` — writes new ADRs in standard format
5. `onboard` — ingests ADRs as context for new contributors

The agent loop: **Read → Validate → Capture**. Agents do not receive raw instructions; they receive structured ADRs and run TypeScript checks to confirm compliance.

**Gap.** Archgate requires ADRs to be pre-authored by humans. It does NOT auto-extract decisions from session transcripts or generate ADRs from agent conversations. The agent reads the intent; it does not record new intent.

Sources: [Archgate CLI docs](https://cli.archgate.dev/) · search results

---

### 1.4 Kiro / Tessl — Spec File Architecture

**Kiro spec format.** Three Markdown files per spec, stored in `.kiro/specs/<spec-name>/`:

```
.kiro/specs/user-auth/
  requirements.md    # User stories: "As a... GIVEN/WHEN/THEN"
  design.md          # Technical architecture, sequence diagrams, data models
  tasks.md           # Discrete implementation tasks with completion tracking
```

No YAML schema enforcement. Content is freeform Markdown. Tasks link to requirements via human-written cross-references.

**Tessl spec-as-source model.** Uses semantic tags:
- `@generate` — marks a section that drives code generation
- `@test` — marks a section that drives test generation

Generated code is annotated `// GENERATED FROM SPEC - DO NOT EDIT`. The spec file is the authoritative artifact; code is a 1:1 derivative. Tessl is the only tool currently pursuing "spec-as-source" (the spec IS the primary artifact; code is generated output).

**Spec-anchored vs. spec-first vs. spec-as-source (from Martin Fowler's analysis):**

| Level | Human edits | Agent role | Intent location |
|-------|-------------|------------|----------------|
| Spec-first | Spec then discards | Implements spec | Session chat (lost) |
| Spec-anchored | Spec persists | Implements + maintains spec | Spec file (preserved) |
| Spec-as-source | Spec only | Generates + regenerates code | Spec file (primary artifact) |

**Spec drift detection.** No tool in this category currently ships automated spec-vs-code drift detection as a first-class feature. Zencoder's Zenflow claims "enforce rules, prevent drift" but implementation details are not published. The gap is real.

Sources: [Kiro docs](https://kiro.dev/docs/specs/) · [Martin Fowler SDD analysis](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html) · [Tessl blog](https://tessl.io/blog/spec-driven-development-10-things-you-need-to-know-about-specs/)

---

## 2. Academic Research Missed in Pass 1

### 2.1 Prompt Provenance Model (PPM)

Procko, Vonder Haar, Elvira, Ochoa (October 2025, SSRN) introduced the **Prompt Provenance Model (PPM)**, a conceptual model for representing the lineage of prompts, completions, and dialogue histories using the W3C PROV framework.

**Core claim.** Capturing prompt-level provenance is essential for auditability, explainability, and regulatory compliance in LLM ecosystems. Applications: research reproducibility, model debugging, forensic accountability.

**PROV mapping.** The W3C PROV data model uses three entity types:
- `Entity` — a prompt, completion, or dialogue state
- `Activity` — an LLM inference call
- `Agent` — a user, software agent, or model

Relationships:
- `wasGeneratedBy(completion, inference_call)` — completion was produced by this call
- `used(inference_call, prompt)` — the inference used this prompt as input
- `wasDerivedFrom(prompt_v2, prompt_v1)` — prompt was revised from an earlier version
- `wasAttributedTo(completion, model)` — completion is attributed to this model

**For code decisions.** A prompt lineage graph would show: original request → refined prompt → tool call → result → code written. Each node is traceable. The framework does not yet have tooling for automatic extraction — it is a data model, not an implementation.

Source: [SSRN Prompt Provenance Model](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=5682942)

---

### 2.2 Code Digital Twin (arXiv 2503.07967, 2026)

**Problem.** Current RAG-based AI coding tools fail on ultra-complex enterprise systems because required knowledge is "scattered across artifacts and entangled across time," exceeding LLMs' reliable synthesis capacity.

**Approach.** A "persistent and evolving knowledge infrastructure" coupled to the codebase, comprising three knowledge layers:

1. **Code and Artifact Map** — typed structural representation (files, functions, modules) with `contains`, `imports`, `calls` relationships; anchored to git commits
2. **Functionality-Oriented Skeleton** — domain concepts operationalized through workflows and responsibilities; dependency mappings to code
3. **Rationale Spine** — design decisions, trade-offs, constraints; `justified-by` links to supporting evidence (commit messages, issues, discussions)

**Decision storage schema (Rationale Spine).**
```
Decision {
  id: UUID
  title: string
  context: string
  decision: string
  rationale: string
  alternatives_considered: [{ description, reason_rejected }]
  constraints: [string]
  evidence_links: [{ type: "commit"|"issue"|"discussion", ref: string }]
  affected_components: [ComponentRef]
  created_at: timestamp
  last_updated_at: timestamp
}
```

**4-stage extraction pipeline.**
1. Static/dynamic analysis → Code Map
2. Schema-guided extraction from docs + program analysis → Functionality Skeleton
3. Mining rationale from commits, issues, design discussions → Rationale Spine (AI-assisted)
4. Bidirectional link construction → traceability graph

**Incremental update.** When code changes: detect affected components → refresh knowledge cards → update links to new versions. Human-in-the-loop validation during code reviews.

**Key capability.** Answers queries like "which design decision limits caching scope, and where is it enforced?" by navigating from abstract decision node to concrete implementation files.

Source: [arXiv 2503.07967 Code Digital Twin](https://arxiv.org/abs/2503.07967)

---

### 2.3 Agile V Framework (arXiv 2602.20684, 2026)

**Relevant finding.** The Compliance Auditor agent in this multi-agent engineering framework automatically generates six artifact types as workflow by-products, including a **decision rationale document** — capturing "the full decision rationale — why each design choice was made — generating structured audit-evidence logs in real time."

**State directory structure.**
```
.agile-v/
  config/           # YAML/JSON metadata
  change-log/       # cycle tracking (C1, C2, ...)
  approvals/        # human gate records
  traceability/     # ATM.md — requirements → code → tests
  red-team/         # findings with severity (MAJOR/MINOR)
  validation/       # pass/fail summary per cycle
```

**Scale evidence.** Each cycle: 6 human prompts → 500+ lines of code + 54 tests + full compliance documentation. Decision capture happens as a free by-product of the structured multi-agent workflow — not as a separate human step.

**Transferable pattern for intent preservation.** Agile V demonstrates that AI agents can generate decision rationale automatically IF the workflow is structured to require it. The Compliance Auditor role is explicitly tasked with rationale capture, not left implicit. This is the key insight: intent preservation requires a dedicated agent role, not retrofitting onto existing agents.

Source: [arXiv 2602.20684 Agile V](https://arxiv.org/html/2602.20684)

---

### 2.4 AgenticAKM — Multi-Agent ADR Extraction (arXiv 2602.04445, 2026)

**Architecture for extracting ADRs from codebases.**

Four specialized agent groups:
1. **Architecture Extractor**: parses repository, creates architecture summary
2. **Architecture Retriever**: finds existing documentation, prior decisions
3. **Architecture Generator**: drafts new ADRs using summaries
4. **Architecture Validator**: quality-checks outputs; loops up to 3× if failing

**Inputs.** Code repository + commit histories + issue tracker (Jira) + existing documentation.

**ADR output format.** Standard Markdown with sections: Title, Status, Context, Decision, Consequences.

**Quality results.** User study on 29 repositories: agentic approach scored 3.9 vs. 3.3 for single-prompt LLM — notably stronger on completeness and "more reflective of actual architectural reasoning."

**Critical gap.** This operates on existing code+documentation, not on live agent sessions. It reconstructs past decisions from artifacts — it does not capture decisions as they are made.

Source: [arXiv 2602.04445 AgenticAKM](https://arxiv.org/html/2602.04445v1)

---

### 2.5 LLM ADR Generation — Accuracy Baseline (arXiv 2403.01709)

**Dataset.** 95 ADRs from 5 open-source repositories.

**Best performance.** GPT-4 zero-shot: BERTScore F1 = 0.849. Not sufficient for production without human review.

**Key finding.** Fine-tuning Flan-T5-base (248M parameters) achieved results comparable to GPT-3.5 (175B parameters) — 700x efficiency gain. Smaller, locally-deployable models are viable for ADR generation if fine-tuned on domain data.

**Implications for building an intent extraction tool.** Do NOT use raw GPT-4 zero-shot for production decision extraction. Fine-tune a small model (Flan-T5-base or equivalent) on a curated dataset of (conversation, decision) pairs. The threshold investment is creating ~200-500 high-quality labeled examples.

Source: [arXiv 2403.01709 LLM ADR Generation](https://arxiv.org/abs/2403.01709)

---

### 2.6 Bonsai IDE — Prompt Lineage Tracking (arXiv 2503.02833)

**Concept.** Treats AI-generated code as "a dynamic collection of snippets linked to the prompts from which they were generated." Proposes:

1. **Code Generation Graphs** — non-linear relationships between code snippets and originating prompts
2. **Interactive Code Evolution Timelines** — from initial generation through finalized versions
3. **Regeneration Networks** — how different prompts diverge into alternative implementation paths
4. **Prompt lineage tracking algorithms** — track and visualize prompt-to-code transformations

**Current status.** Conceptual/research stage. No shipped implementation. But the data model is well-specified and directly applicable.

**Key research direction.** "Developing prompt lineage tracking algorithms to track and visualize transformations from prompts to executable code." This is the exact technical challenge for intent preservation.

Source: [arXiv 2503.02833 Bonsai IDE](https://arxiv.org/html/2503.02833)

---

## 3. Adjacent Industry Solutions

### 3.1 Aerospace — DO-178C Traceability

DO-178C (Software Considerations in Airborne Systems and Equipment Certification) mandates bidirectional traceability for safety-critical airborne software:
- High-level requirements → Low-level requirements → Source code → Object code → Structural coverage tests

**Tools in production use:**
- IBM DOORS / DOORS Next Generation — requirements management with unique IDs, version history, formal links
- Siemens Polarion — every artifact stored in version control; every modification generates a Version History Record
- Parasoft DTP — correlates unique IDs from DOORS with static analysis findings, code coverage, test results

**What the regulated aerospace industry has that software dev lacks:**
1. Every requirement has a unique, permanent ID
2. Every code element is linked to ≥1 requirement ID
3. Links are versioned — you can see which requirement a piece of code traced to at any point in time
4. Change impact analysis is a formal step: changing requirement X requires re-verifying all code linked to X

**Transferable pattern.** The `decision ID` is the missing primitive in AI-assisted development. Assign a `DEC-0001` to every design decision made during an agent session. Link code to decision IDs. This is what Archgate's ADR IDs (e.g., `ARCH-003`) partially implement, but without the code-linkage side.

Sources: [Parasoft DO-178C traceability](https://www.parasoft.com/learning-center/do-178c/requirements-traceability/) · [Polarion requirements](https://polarion.plm.automation.siemens.com/products/polarion-requirements)

---

### 3.2 FDA 21 CFR Part 11 — Pharmaceutical Audit Trails

FDA 21 CFR Part 11 requires electronic records to be: trustworthy, reliable, equivalent to paper records. For software development in regulated industries this means:

- Secure, computer-generated, time-stamped records of every creation/modification/deletion
- Chronological log of actions with accountability (who did what when)
- Tamper-evident — records cannot be altered without detection

**Implementation.** Audit trail platforms (Veeva Vault, OpenText, DISCO) store: actor, action, before-value, after-value, timestamp, system context. Every change is append-only; modifications create new versions, not overwrites.

**Transferable pattern.** Append-only JSONL (as used by Claude Code's transcript format) is architecturally equivalent to 21 CFR Part 11 audit trails. The structure is there. What's missing is the semantic layer: which entries represent decisions vs. tool calls vs. exploratory iterations.

Sources: [FDA 21 CFR Part 11](https://simplerqms.com/21-cfr-part-11-audit-trail/) · [SimplerQMS audit trail]

---

### 3.3 Legal Tech — eDiscovery Decision Provenance

eDiscovery platforms (Everlaw, DISCO, Logikcull) face a structurally similar problem: massive conversation/document corpora, need to extract specific decisions and establish accountability chains.

**2025-2026 state:** AI capabilities are "mandatory requirements for 89% of firms." Key techniques:
- Document clustering to group related decision threads
- Semantic search across millions of documents for decision-relevant content
- Chronological timeline reconstruction from scattered sources
- Defensibility through audit trails (chain of custody documentation)

**Key transferable insight.** eDiscovery treats finding "who decided X when" as a retrieval problem, not a capture problem. They accept that decisions are not structured at creation time; they build extraction tools that work backward from unstructured corpora. This is the correct model for retroactive intent extraction from AI session transcripts.

Sources: [eDiscovery 2025 trends](https://trustarray.com/en-us/insights/articles/5-discovery-trends-that-transformed-legal-work-in-2025)

---

### 3.4 MLOps — Experiment Tracking as Structural Analog

W&B, MLflow, Neptune track: hyperparameters, dataset versions, metrics, model artifacts, environment snapshots. Every experiment run is a first-class entity with a unique ID, linked to: code version (git hash), data version, configuration, outputs.

**The W3C PROV mapping for ML experiments** (yProv4ML library):
- `Entity`: model checkpoint, dataset version, parameter set
- `Activity`: training run
- `Agent`: researcher, script
- `wasGeneratedBy(checkpoint, training_run)`
- `used(training_run, dataset_v3, config_v2)`

**What software engineering lacks that MLOps has:**
1. First-class "experiment" entity — every significant attempt is tracked separately, not overwritten
2. Parameter logging — the "why I tried this" is encoded in the parameter values themselves
3. Metric comparison — you can compare run A vs. run B and see exactly what changed

**Direct translation for intent preservation:**
```
Experiment → Agent session (UUID)
Hyperparameters → Prompt + constraints given to agent
Metrics → Code quality metrics (tests passed, coverage, lint)
Checkpoint → Commit SHA after session
Dataset version → Codebase state at session start (git hash)
```

An intent-tracking system that mirrors MLOps experiment tracking would log: `(session_id, initial_prompt, constraints, resulting_commit, quality_metrics)`. This already exists in fragmentary form across Claude Code transcripts + git history. The gap is integration.

Sources: [yProv4ML paper](https://arxiv.org/html/2507.01075v1) · [MLOps principles](https://ml-ops.org/content/mlops-principles)

---

## 4. Concrete Implementation Patterns

### 4.1 Claude Code JSONL Transcript — Complete Schema

Every session is stored at `~/.claude/projects/<project-hash>/<session-uuid>.jsonl`. One JSON record per line, append-only.

**Base record fields (all message types):**
```json
{
  "type": "user|assistant|file-history-snapshot|queue-operation|summary",
  "uuid": "string (UUID4)",
  "parentUuid": "string|null",
  "sessionId": "string (UUID4)",
  "isSidechain": false,
  "isMeta": false,
  "cwd": "/absolute/path",
  "timestamp": "2026-03-12T14:22:35.755Z",
  "version": "claude-code-2.x.x"
}
```

**User message additional fields:**
```json
{
  "userType": "external",
  "gitBranch": "main",
  "thinkingMetadata": {
    "level": "high|medium|low",
    "disabled": false,
    "triggers": []
  },
  "todos": [],
  "slug": "implement-auth-flow",
  "message": {
    "role": "user",
    "content": "string or array"
  }
}
```

**Assistant message additional fields:**
```json
{
  "requestId": "req_011CWfFS...",
  "agentId": "optional: short ID for subagent",
  "slug": "optional: session slug",
  "message": {
    "role": "assistant",
    "model": "claude-sonnet-4-6",
    "content": [/* content blocks */],
    "stop_reason": "end_turn|tool_use",
    "usage": {
      "input_tokens": 12847,
      "output_tokens": 834,
      "cache_creation_input_tokens": 0,
      "cache_read_input_tokens": 11200
    }
  }
}
```

**Content block types in assistant messages:**
```json
{ "type": "text", "text": "Here is the implementation..." }

{ "type": "thinking", "thinking": "Let me consider the tradeoffs...", "signature": "hash" }

{ "type": "tool_use", "id": "toolu_01ABC...", "name": "Write", "input": {
    "file_path": "/path/to/file.py",
    "content": "..."
}}
```

**Tool result in user message (response to tool_use):**
```json
{
  "type": "tool_result",
  "tool_use_id": "toolu_01ABC...",
  "content": "execution output or error text",
  "is_error": false
}
```

**File history snapshot (tracks file changes per session):**
```json
{
  "type": "file-history-snapshot",
  "messageId": "uuid of associated assistant message",
  "snapshot": {
    "trackedFileBackups": {
      "src/auth.py": {
        "backupFileName": "abc123@v2",
        "version": 2,
        "backupTime": "2026-03-12T14:22:36.000Z"
      }
    }
  }
}
```

**The `thinking` block is the primary intent signal.** When extended thinking is enabled, Claude's internal reasoning before writing code is captured verbatim in the JSONL. This is the richest source of decision rationale — more detailed than the response text, includes alternatives considered, trade-offs weighed.

**Graph structure.** `parentUuid` links form a DAG. Subagents create new session files; the parent session references them. Every branching, retry, and subagent spawn is traceable.

Sources: [Claude Code session schema gist](https://gist.github.com/samkeen/dc6a9771a78d1ecee7eb9ec1307f1b52) · [DuckDB analysis](https://liambx.com/blog/claude-code-log-analysis-with-duckdb) · [Claude Code transcripts repo](https://github.com/simonw/claude-code-transcripts)

---

### 4.2 Claude Code Hooks — Which Events to Intercept for Intent

**Complete event table (as of Claude Code with hook system v2, 2026):**

| Event | When | Blocking | Intent-relevant |
|-------|------|----------|----------------|
| `SessionStart` | Session begins/resumes | No | Yes — load prior decisions as context |
| `UserPromptSubmit` | User submits prompt | Yes | Yes — capture intent statement |
| `PreToolUse` | Before tool executes | Yes | Yes — capture decision to act |
| `PostToolUse` | After tool succeeds | No | Yes — capture outcome |
| `PostToolUseFailure` | After tool fails | No | Yes — capture failure rationale |
| `Stop` | Claude finishes responding | Yes | Yes — extract session summary |
| `SubagentStart` | Subagent spawned | No | Yes — capture delegation decision |
| `SubagentStop` | Subagent finishes | Yes | Yes — capture subagent outcome |
| `PreCompact` | Before context compaction | No | Critical — capture before context lost |
| `SessionEnd` | Session terminates | No | Yes — final persistence |
| `InstructionsLoaded` | CLAUDE.md loaded | No | Audit only |
| `WorktreeCreate/Remove` | Worktree lifecycle | Yes/No | Audit only |
| `TeammateIdle` | Team agent goes idle | Yes | Yes — capture agent pause rationale |
| `TaskCompleted` | Task marked complete | Yes | Yes — validate against intent |

**For intent capture, the critical hooks are:**

1. **`UserPromptSubmit`** — the raw intent statement. Receives `{ prompt: string }`. This IS the intent. Extract and store immediately.

2. **`Stop`** — session turn complete. Use this to run async decision extraction against the `transcript_path` JSONL.

3. **`PreCompact`** — fires before context window compaction. This is the last chance to preserve intent before the conversation is summarized/truncated. Any decisions made in the compacted portion are at risk.

4. **`SessionEnd`** — final write opportunity. Store the session summary with structured decisions extracted.

**Hook input common fields available for intent capture:**
```json
{
  "session_id": "abc123",
  "transcript_path": "/path/to/session.jsonl",
  "cwd": "/project/root",
  "hook_event_name": "Stop"
}
```

The `transcript_path` is the key field — it gives direct access to the full session JSONL from within any hook, enabling extraction of all decisions made during the session.

**`additionalContext` output field on `SessionStart`.** When returning from a `SessionStart` hook, you can inject text into Claude's context before it processes anything. This is the mechanism for loading prior session decisions at session start:

```json
{
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "additionalContext": "Prior decisions in this project:\n- DEC-001: Use SQLite not PostgreSQL (cost, deployment simplicity)\n- DEC-002: JWT over sessions (stateless scaling requirement)\n"
  }
}
```

**Hook handler types** (all usable for intent capture):
- `command` — shell script, receives JSON on stdin
- `http` — POST to local service (e.g., an intent MCP server)
- `prompt` — single-turn Claude evaluation (use for decision extraction)
- `agent` — spawns subagent with tools (use for complex extraction + storage)

Sources: [Claude Code hooks reference](https://code.claude.com/docs/en/hooks) · hooks file cached at tool output

---

### 4.3 Automatic Decision Extraction Pipeline — How to Build It

**Input.** A Claude Code session JSONL file (one session, potentially hundreds of records).

**Target output.** A structured decision record per significant decision made during the session:
```json
{
  "decision_id": "DEC-20260312-001",
  "session_id": "abc123",
  "timestamp": "2026-03-12T14:30:00Z",
  "decision": "Use SQLite for local state storage",
  "rationale": "PostgreSQL adds deployment complexity; SQLite sufficient for single-user tool",
  "alternatives_rejected": [
    { "option": "PostgreSQL", "reason": "Overcomplicated for local tool" },
    { "option": "JSON files", "reason": "No query capability" }
  ],
  "confidence": 0.87,
  "source_message_uuid": "709290a1-...",
  "source_thinking_block": true,
  "affected_files": ["src/storage.py"],
  "tags": ["architecture", "data-storage"]
}
```

**NLP pipeline (4 stages):**

**Stage 1 — Candidate identification.** Classify each assistant message as: decision, exploration, implementation, verification, or other. Use a lightweight classifier (DistilBERT or fine-tuned Flan-T5-base). Key signals:
- Presence of `thinking` blocks (strong signal — explicit deliberation)
- Phrases: "I'll use X instead of Y", "because", "the reason", "this approach", "trade-off"
- Tool calls following a deliberation block (decision → action pattern)

**Stage 2 — Decision extraction.** For messages classified as decisions, run structured extraction with a prompt template:
```
Extract the architectural decision from this AI coding session excerpt.

Format:
{
  "decision": "<what was decided in one sentence>",
  "rationale": "<why this was chosen>",
  "alternatives_rejected": [{"option": "...", "reason": "..."}],
  "confidence": <0-1>
}

Excerpt:
<thinking block + assistant response text>
```

**Stage 3 — File linkage.** Correlate extracted decisions with the `file-history-snapshot` records following the decision message. This gives the `affected_files` array — connecting the decision to the code it produced.

**Stage 4 — Deduplication and ranking.** Sessions often re-discuss the same decision. Cluster by semantic similarity (sentence embeddings); keep the most explicit statement of each decision.

**Cost estimate (2026 pricing).**
- Typical session: 20-80 assistant messages, ~50K tokens total
- Stage 1 classification: local model, near-zero cost
- Stage 2 extraction: 5-15 decision candidates per session
- At claude-haiku-4.5 ($1/$5 per MTok): ~5K input tokens per extraction call × 10 decisions = 50K tokens = $0.05 per session
- Using cached context for long sessions: drops to ~$0.01-0.02 per session
- At 10 sessions/day: ~$0.15/day or ~$4.50/month

**Latency.** Extraction runs async at `Stop` hook. With `async: true` on the hook, it does not block Claude's next response. Total extraction time: 2-5 seconds per session.

Sources: [Google intent decomposition research](https://research.google/blog/small-models-big-results-achieving-superior-intent-extraction-through-decomposition/) · [arXiv 2403.01709 ADR accuracy baselines](https://arxiv.org/abs/2403.01709) · Claude pricing

---

### 4.4 Git Storage Options — Tradeoffs for Intent Metadata

Three mechanisms for storing intent alongside commits, with tradeoffs:

#### Option A: Git Notes (`refs/notes/decisions`)
```bash
git notes --ref=decisions add -m '{
  "decisions": [{"id": "DEC-001", "text": "Use SQLite", "session": "abc123"}],
  "session_id": "abc123",
  "extracted_at": "2026-03-12T14:30:00Z"
}' HEAD
```

| Aspect | Detail |
|--------|--------|
| Commit SHA | Unchanged — notes don't modify commits |
| Push | Must explicitly `git push origin refs/notes/decisions` |
| GitHub display | GitHub stopped displaying notes in 2014 — invisible in UI |
| Rebase survival | Lost by default unless `git config notes.rewriteRef refs/notes/decisions` |
| Size limit | Notes can store arbitrary blobs; no practical size limit |
| Tooling | Requires custom tooling to read/search — `git notes --ref=decisions show HEAD` |
| Best for | Large structured payloads; preservation after-the-fact; CI/CD annotation |

#### Option B: Git Trailers (in commit message)
```
feat: implement auth module

Use JWT over session cookies for stateless scaling.

Decision-Id: DEC-20260312-001
Decision-Rationale: JWT enables horizontal scaling without shared session store
Decision-Session: abc123
AI-Model: claude-sonnet-4-6
Prompt-Hash: sha256:deadbeef
```

| Aspect | Detail |
|--------|--------|
| Commit SHA | Modified — trailers are part of commit message |
| Push | Normal `git push` |
| GitHub display | Rendered in commit view as key-value pairs |
| Rebase survival | Preserved if rebasing preserves message |
| Size limit | Practical limit ~1KB (commit messages should be short) |
| Tooling | `git log --pretty=format:"%s%n%b" | grep "Decision-"` or `git interpret-trailers` |
| Best for | Metadata that should be visible in commit log; human-readable signals |

#### Option C: Sidecar File (`.decisions/` directory)
```
.decisions/
  2026-03-12-abc123.json    # one file per session
  index.json                # searchable index
```

| Aspect | Detail |
|--------|--------|
| Commit SHA | Unchanged (decisions are in separate files) |
| Push | Normal `git push` |
| GitHub display | Visible as normal files |
| Rebase survival | Preserved as long as `.decisions/` is committed |
| Size limit | None — full JSON files |
| Tooling | Any JSON tooling; standard file search |
| Best for | Rich structured data; cross-session search; linkage to multiple commits |

**Recommendation for an intent preservation tool.** Combine Option B (trailers) and Option C:
- Trailers: short `Decision-Id: DEC-001` linking commit to decision record
- Sidecar `.decisions/`: full structured JSON with rationale, alternatives, confidence

This makes decisions visible in git log (via trailers) while storing full data where there are no size constraints.

Sources: [Git Notes feature](https://risadams.com/blog/2025/04/17/git-notes/) · [Git Trailers reference](https://alchemists.io/articles/git_trailers) · [AI attribution debate](https://bence.ferdinandy.com/2025/12/29/dont-abuse-co-authored-by-for-marking-ai-assistance/)

---

### 4.5 Intent MCP Server — Architectural Sketch

An MCP server that provides intent context to AI agents. No such tool ships today — this is the gap.

**MCP server interface (tools to expose):**

```typescript
// Tool: record_decision
{
  name: "record_decision",
  description: "Record an architectural decision made during this session",
  inputSchema: {
    decision: "string — what was decided",
    rationale: "string — why",
    alternatives: "array of {option, reason_rejected}",
    tags: "array of strings",
    affected_files: "array of file paths"
  }
}

// Tool: query_decisions
{
  name: "query_decisions",
  description: "Find relevant past decisions for the current task",
  inputSchema: {
    query: "string — semantic search query",
    files: "optional array — filter by affected files",
    tags: "optional array — filter by tags",
    since: "optional date — decisions after this date"
  }
}

// Tool: get_decision_context
{
  name: "get_decision_context",
  description: "Get all decisions relevant to a specific file",
  inputSchema: {
    file_path: "string"
  }
}
```

**Storage backend.** SQLite with FTS5 for full-text search. Schema:
```sql
CREATE TABLE decisions (
  id TEXT PRIMARY KEY,           -- "DEC-20260312-001"
  session_id TEXT,
  commit_sha TEXT,
  decision TEXT NOT NULL,
  rationale TEXT,
  alternatives JSON,             -- [{option, reason_rejected}]
  confidence REAL,
  source_message_uuid TEXT,
  affected_files JSON,           -- ["/path/to/file"]
  tags JSON,                     -- ["architecture", "security"]
  created_at TIMESTAMP,
  project_root TEXT
);

CREATE VIRTUAL TABLE decisions_fts USING fts5(
  decision, rationale, tags,
  content=decisions, content_rowid=rowid
);
```

**Integration with Claude Code hooks.**
1. `SessionStart` hook → call `query_decisions` with session context → inject as `additionalContext`
2. `Stop` hook → async extraction pipeline → call `record_decision` for each extracted decision
3. `PreCompact` hook → snapshot current turn's decisions before compaction

**For agents in Archgate-style workflows:** expose `query_decisions` and `get_decision_context` via MCP. Agents call these before writing code, getting structured decision context without needing to parse raw session transcripts.

---

## 5. Quantitative Data

### 5.1 How Many Decisions Per Session

**Estimate based on session structure.**

A typical Claude Code session for a non-trivial feature:
- 20-80 user prompts
- 20-80 assistant responses
- 50-200 tool calls (Read, Edit, Write, Bash, WebSearch)
- Token range: 50K-500K tokens (per Faros AI analysis)

**Decision density estimate.** From the AgenticAKM study: 95 ADRs from 5 repositories covering months of development. Rough rate: 1-2 significant architectural decisions per feature. But micro-decisions (which function name, which error handling approach) occur 5-15× per session.

Classification:
| Decision type | Frequency per session | Intent preservation value |
|--------------|----------------------|--------------------------|
| Architectural (library choice, data model) | 0-3 | Very high |
| Design (API shape, error handling strategy) | 1-5 | High |
| Implementation (loop vs. recursion, variable name) | 5-20 | Low-medium |
| Exploratory (trying approach, abandoning) | 5-15 | Medium (why it failed is valuable) |

**Actionable estimate.** A typical 45-minute feature session yields 2-8 decisions worth preserving, out of 10-40 that an extraction model would find. Signal-to-noise ratio requires a confidence threshold (~0.7) and tagging to identify architectural vs. implementation decisions.

### 5.2 Cost of Retrospective Extraction Per Session

**Per session extraction cost:**
- Session JSONL reading: free (local file)
- Classification pass (local model): ~$0
- Extraction calls (haiku-4.5, 5K tokens × 8 decisions): ~$0.04
- Storage write: ~$0
- **Total: $0.04-0.08 per session**

At 10 sessions/day across a team: ~$0.50/day or ~$15/month — within "invisible cost" territory.

**With prompt caching** (session context is identical across classification calls): 90% cost reduction → ~$1.50/month per active developer.

### 5.3 Developer Context Loss — Quantified

From the METR 2025 randomized controlled trial (16 experienced developers, 246 tasks):
- AI tools increased task completion time by **19%** — slowed developers down
- Root cause: "extra cognitive load and context-switching" disrupting flow
- 9% of total task time spent specifically reviewing/modifying AI-generated code

From Qodo State of AI Code Quality 2025:
- **65% of developers report missing context** as the top issue during refactoring
- ~60% report it during test generation and code review

From Augment Code (Augment Intent docs):
- Traditional AI tools operate in 4K-8K token windows, forcing manual re-segmentation
- Developers must re-explain architectural patterns at the start of each session

**Structural finding.** The 19% productivity loss is not from bad code generation — it is from the overhead of establishing and re-establishing context. Intent preservation directly attacks this overhead. If decisions are loaded at `SessionStart`, the context reconstruction cost approaches zero.

Sources: [METR 2025 study](https://arxiv.org/abs/2507.09089) · [Augment Code productivity analysis](https://www.augmentcode.com/guides/why-ai-coding-tools-make-experienced-developers-19-slower-and-how-to-fix-it) · [Qodo AI code quality report](https://www.qodo.ai/reports/state-of-ai-code-quality/)

### 5.4 Session Token Scale

From Faros AI Claude Code token analysis (2026):
- Pro users: ~44K tokens per 5-hour window
- Max 5× users: ~88K tokens
- Max 20× users: ~220K tokens
- Individual query average (pre-optimization): ~8,200 input tokens
- Claude Code uses 5.5× fewer tokens than Cursor for equivalent tasks

A 220K-token session contains substantial thinking blocks (Claude's internal deliberation). At ~20% overhead for thinking, that is ~44K tokens of internal reasoning — the primary source of decision rationale that is currently discarded.

Sources: [Faros AI token limits](https://www.faros.ai/blog/claude-code-token-limits) · [Claude Code vs. Cursor benchmarks](https://render.com/blog/ai-coding-agents-benchmark)

---

## 6. W3C PROV — Formal Framework for Code Decision Lineage

The W3C PROV data model is the most rigorous existing framework for representing software decision provenance. It is used in scientific workflows (yProv4ML), data lineage (TableVault), and now being applied to LLM interactions (Prompt Provenance Model).

**PROV core entities for code decisions:**

```turtle
# In RDF/Turtle notation

:decision_DEC001 a prov:Entity ;
  prov:wasGeneratedBy :session_abc123 ;
  prov:wasAttributedTo :agent_claude-sonnet-4-6 ;
  :hasDecisionText "Use SQLite for local storage" ;
  :hasRationale "Lower deployment complexity" .

:session_abc123 a prov:Activity ;
  prov:startedAtTime "2026-03-12T14:00:00Z"^^xsd:dateTime ;
  prov:endedAtTime "2026-03-12T15:00:00Z"^^xsd:dateTime ;
  prov:wasAssociatedWith :human_developer, :agent_claude-sonnet-4-6 .

:commit_abc456 a prov:Entity ;
  prov:wasDerivedFrom :decision_DEC001 ;
  prov:wasGeneratedBy :session_abc123 ;
  :hasGitSHA "abc456..." .

:file_storage_py a prov:Entity ;
  prov:wasDerivedFrom :decision_DEC001 ;
  prov:wasRevisionOf :file_storage_py_v1 .
```

**DeCPROV** is a PROV extension specifically for decisions, offering `wasDecidedBy`, `hadDecisionContext`, `consideredAlternative` predicates. This is the most precise existing vocabulary for the intent preservation problem.

**Practical implication.** An intent MCP server could store decisions as PROV-compliant JSON-LD, making them interoperable with any PROV-aware tooling (Provenance Explorer, ProvStore, custom SPARQL endpoints). This is overkill for day-1 implementation but provides a standards path for enterprise adoption.

Sources: [W3C PROV Overview](https://www.w3.org/TR/prov-overview/) · [DeCPROV](https://www.w3.org/2001/sw/wiki/PROV) · [yProv4ML](https://arxiv.org/html/2507.01075v1) · [Prompt Provenance Model](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=5682942)

---

## 7. Summary of Implementation-Ready Findings

| Finding | Actionability |
|---------|--------------|
| Claude Code JSONL `thinking` blocks contain verbatim agent deliberation including alternatives considered | Extract immediately — richest intent signal, currently discarded |
| `PreCompact` hook fires before context is compacted/truncated — last chance to persist intent | Implement as first hook — prevent decision loss at window boundary |
| `SessionStart` hook + `additionalContext` output injects prior decisions into context | Implement second — closes the cross-session continuity gap |
| Fine-tuned Flan-T5-base (248M params) achieves GPT-3.5-level ADR generation — locally deployable | Use for decision extraction in privacy-sensitive contexts |
| Decision extraction cost: ~$0.04-0.08/session — effectively free | No cost barrier to implementing |
| W3C PROV / DeCPROV provides standards-based vocabulary for decision lineage | Use for schema design; enables enterprise interoperability |
| Agile V demonstrates that a "Compliance Auditor" agent role generates rationale as workflow by-product | Architect a dedicated intent-capture agent for multi-agent pipelines |
| Git trailer `Decision-Id:` + sidecar `.decisions/` JSON is the optimal storage hybrid | Visibility in git log + full structured data without size limits |
| AgenticAKM (scoring 3.9 vs 3.3) shows multi-agent extraction outperforms single-prompt | Use 4-agent pipeline (extractor, retriever, generator, validator) for ADR quality |
| 65% of developers report missing context as top AI coding issue | Intent preservation directly addresses the #1 reported pain point |
