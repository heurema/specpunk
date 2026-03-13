---
title: Intent Preservation in AI-Assisted Programming
date: 2026-03-12
source: web-research
type: deep-research
tags: [intent-preservation, AI-coding, spec-driven-development, cognitive-debt, ADR, design-rationale]
---

# Intent Preservation in AI-Assisted Programming

## Context

Andrey Breslav (creator of Kotlin, founder of CodeSpeak) articulated a structural gap in AI-assisted development: when developers use coding agents, the human intent encoded in prompts is discarded after code generation. Teams see the resulting code, not the reasoning behind it. The human spoke to the machine in natural language; they communicate with teammates in code. This research maps the problem space, existing tools, academic framing, and solution trajectories.

---

## 1. The Problem

### 1.1 The Intent Evaporation Gap

Guy Podjarny (CEO, Tessl) names the core issue directly:

> "Code is a representation of an intent — it's a choice. The problem is that once code exists, the 'why' behind those decisions evaporates."

Alan Pope describes the practical consequence:

> "I come back to code months, or maybe years later, and think, 'oh, I need to make some adjustments to that. And, I have completely forgotten about all the choices that I made.'"

In traditional development this was already a problem. AI-assisted coding at speed makes it acute: agents make dozens of micro-decisions per session — library selection, error handling strategy, data structure layout — none of which are recorded anywhere.

### 1.2 Cognitive Debt

Margaret-Anne Storey (UVic) introduced the term **cognitive debt** in February 2026 to distinguish it from technical debt:

> "A program is more than its source code — it is a theory living in developers' minds, capturing what the program does, how developer intentions are implemented."

She cites Peter Naur's 1985 insight that a program is not its source code but a theory held by its builders. When AI agents produce code faster than developers can internalize it, the theory is never formed. The result: teams accumulate cognitive debt — they operate the system without understanding it. Even if the generated code is clean, the humans involved "may have simply lost the plot."

Storey's three warning signs: hesitation about making modifications, over-reliance on tribal knowledge, treating the system as a black box.

Source: [margaretstorey.com — Cognitive Debt (Feb 2026)](https://margaretstorey.com/blog/2026/02/09/cognitive-debt/)

### 1.3 Quantified Team Pain

From Qodo's State of AI Code Quality 2025 report and CodeRabbit's December 2025 analysis:

- AI code has **1.7× more major issues** (logic errors, race conditions, security vulnerabilities) than human-written code
- Teams with high AI adoption complete 21% more tasks and merge 98% more PRs, but **median PR review time rises 91%** — review becomes the bottleneck
- The most common complaint is not hallucination but **relevance**: AI suggestions lack the context to understand the ripple effects of changes across a codebase
- Tools that added comments without reducing review effort "were ignored or turned off"

From VentureBeat (2025): AI coding agents have brittle context windows — each session starts without awareness of prior sessions, established conventions, or past mistakes.

### 1.4 The Multi-Agent Visibility Problem

Anthropic's 2026 Agentic Coding Trends Report identifies multi-agent parallelism as the dominant architecture shift, but notes that as agents work longer (hours to days) and make more autonomous decisions, the mechanisms for preserving intent and surfacing reasoning "remain implied rather than explicit." This is the structural gap: more agent autonomy → more invisible decisions → more intent loss.

Source: [Anthropic 2026 Agentic Coding Trends Report](https://resources.anthropic.com/2026-agentic-coding-trends-report)

---

## 2. Historical and Academic Roots

### 2.1 Literate Programming (Knuth, 1984)

Donald Knuth's literate programming paradigm treats programs as literature: natural language explanations are primary; code is embedded within them. The system produces two outputs from one source: "tangled" (machine-readable) code and "woven" (human-readable) documentation.

The explicit goal was intent preservation. Knuth used WEB for TeX and Metafont specifically to preserve the mathematical reasoning and intended algorithms — so future maintainers could understand not just what was implemented but why it was designed that way.

The paradigm failed to achieve mass adoption due to toolchain friction and workflow mismatch, but the problem it identified — code strips out the human reasoning layer — remains the same problem Breslav is attacking with CodeSpeak.

Sources: [Knuth: Literate Programming (Stanford)](https://www-cs-faculty.stanford.edu/~knuth/lp.html) · [Wikipedia: Literate Programming](https://en.wikipedia.org/wiki/Literate_programming)

### 2.2 Design Rationale Capture Research

Academic work on design rationale capture (IBM Research, 1990s–2000s) developed structured formats (IBIS, QOC, DRL) for documenting not just decisions but the alternatives considered and the criteria used. The Cambridge Core paper "Augmenting design patterns with design rationale" (2025) extends this to software patterns. The field identified the same core problem: decisions are made implicitly in conversation and never recorded in code.

The pattern: rationale capture systems were built, failed to achieve adoption (high friction, low perceived value during development), and the rationale evaporated anyway. This is a cautionary data point for any intent-preservation solution — the tooling must integrate into the flow, not interrupt it.

Source: [Cambridge Core: Augmenting design patterns with design rationale](https://www.cambridge.org/core/journals/ai-edam/article/abs/augmenting-design-patterns-with-design-rationale/7A18F03D93429E1A5399DF69BAFB2469)

### 2.3 Intent-Preserving Refactoring (Formal Methods)

Academic research defines **behavior-preserving transformations** as refactorings that change code structure without changing semantics. The "On Preserving the Behavior in Software Refactoring" systematic mapping study (2021, ACM) catalogues 118 approaches. LLM-based refactoring research (ICSE 2025) finds that up to 76.3% of LLM-generated Extract Method suggestions are incorrect due to hallucinations — the model does not preserve intent even when asked to refactor.

The implication: LLMs do not reliably preserve semantic intent even in bounded, well-specified refactoring tasks. For broader intent (architectural decisions, design philosophy), the situation is worse.

Sources: [arXiv:2106.13900 — Behavior Preservation in Refactoring](https://arxiv.org/abs/2106.13900) · [ICSE 2025: LLM-Driven Code Refactoring](https://conf.researchr.org/details/icse-2025/ide-2025-papers/12/LLM-Driven-Code-Refactoring-Opportunities-and-Limitations)

---

## 3. Existing Approaches and Tools

### 3.1 CodeSpeak — Intent as the Language

Andrey Breslav's CodeSpeak (2025) takes the most radical position: replace code with intent. CodeSpeak is a higher-level language that uses plain English with programming language modularity and reuse, compiling to Python, Go, JS/TS. The claim: shrink codebases 5–10× by replacing implementation details (which "the machine knows as well") with declarations of intent (which only humans possess).

The key insight: if intent IS the source artifact, it cannot be lost in translation — because the translation is automated and auditable.

Limitation: CodeSpeak requires developers to reorient from writing code to writing specifications. This is a paradigm shift with unclear adoption friction, and it does not address existing codebases.

Sources: [CodeSpeak website](https://codespeak.dev/) · [The Pragmatic Engineer: The programming language after Kotlin](https://newsletter.pragmaticengineer.com/p/the-programming-language-after-kotlin) · [Cambridge lecture: CodeSpeak](https://www.cst.cam.ac.uk/seminars/list/242185)

### 3.2 Spec-Driven Development (SDD) — Specifications as Living Documents

SDD is the 2025 paradigm answer to vibe coding. Rather than prompting agents ad hoc, developers write structured specifications first; agents implement from them.

**Three tiers** (Birgitta Böckeler, ThoughtWorks, via Martin Fowler):
1. **Spec-first**: Spec guides initial development, then is discarded
2. **Spec-anchored**: Spec persists and evolves with the feature
3. **Spec-as-source**: Spec is the primary artifact; humans never edit generated code

Key tools in the space:

- **Amazon Kiro** (GA November 2025): Three-step workflow (Requirements → Design → Tasks) inside VS Code. Steering files for persistent conventions. Spec-first orientation. Source: [Kiro website](https://kiro.dev/)
- **GitHub Spec-Kit** (open source, September 2024): Multi-file system anchored by a "constitution." Uses per-spec branches. Source: [GitHub Blog: Spec-driven development with AI](https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/)
- **Tessl** (2025): The most intent-focused of the three. `.spec.md` files define what to build; agents generate code to match. Explicit goal: keep intent "that is otherwise scattered and lost in agent conversation histories." Specs are version-controlled, human-readable, and the source of truth for code generation AND team communication. Source: [Tessl website](https://tessl.io/)
- **Augment Intent** (2026): Desktop workspace with coordinator+specialist agent architecture. Living specs auto-update as implementation completes. Source: [Augment Code: Intent](https://www.augmentcode.com/product/intent)

**Critique** (Böckeler via Fowler): Verbose markdown artifacts are tedious to review vs. code. Fixed SDD workflows poorly serve varying problem sizes. LLMs frequently ignore or over-interpret specs despite larger context windows. Model-Driven Development's past failures are a cautionary parallel.

Source: [Martin Fowler: Understanding Spec-Driven-Development](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html)

### 3.3 Architecture Decision Records (ADR) — Decision Capture as Practice

ADRs (Michael Nygard, 2011) are short markdown files that document architectural decisions: context, decision, consequences. They have been a standard practice for capturing "why" alongside "what" in codebases.

**AI-enhanced ADR approaches:**

- **AI ADR writers**: Using LLMs to generate ADRs from existing artifacts (design docs, code), then using a fresh context to draft the detailed record. The key technique: prompt-in-prompt, where a meta-prompt generates a decision-specific prompt. Source: [Equal Experts: Accelerating ADRs with Generative AI](https://www.equalexperts.com/blog/our-thinking/accelerating-architectural-decision-records-adrs-with-generative-ai/)
- **Archgate** (2025): Turns ADRs into executable governance. Each ADR gets a companion `.rules.ts` file. Archgate runs checks in CI and pre-commit hooks. Critical for intent preservation: an MCP server feeds live ADR context to AI coding agents — agents read architectural decisions BEFORE writing code, validate changes against rules, and can automatically capture new decisions back into ADRs. Source: [Archgate](https://archgate.dev/)
- **AGENTS.md vs ADR**: A 2026 article argues that AGENTS.md files (persistent agent instructions) are becoming the new ADR format — not formal enough to be ADRs but serving the same function of preserving project-level intent for agents. Source: [AI Advances: AGENTS.md is the new ADR](https://ai.gopubby.com/agents-md-is-the-ew-architecture-decision-record-adr-3cfb6bdd6f2c/)

### 3.4 Agent Decision Records (AgDR) — Extending ADR for AI Agents

AgDR (me2resh, 2025) extends the ADR format specifically for decisions made BY AI coding agents. The argument: when agents select libraries, choose patterns, or design architecture, those decisions need the same traceability as human decisions.

Key fields beyond standard ADR:
- `agent`: tool name (claude-code, copilot, cursor)
- `model`: specific model version
- `trigger`: what initiated the decision (user-prompt, hook, automation)
- `status`: proposed / executed / superseded

The Y-statement format: "In the context of [situation], facing [concern], I decided [choice] to achieve [goal], accepting [tradeoff]."

Implementation: stored in `docs/agdr/`, integrated as a Claude Code command in `.claude/commands/`.

Source: [GitHub: me2resh/agent-decision-record](https://github.com/me2resh/agent-decision-record)

### 3.5 Git AI — Linking Code to Agent Transcripts

Git AI 1.0 (2025) directly addresses the prompt-to-code lineage gap. The creator identified that Cursor and Claude Code report lines inserted but don't track where those lines end up in the repository, and "the connection between prompt and generated code was immediately lost."

**Technical approach:**
- Coding agents explicitly mark AI-generated code via PreEdit/PostEdit hooks
- Checkpoints capture the diff between human and AI edits
- **Authorship Logs** (stored as Git notes on commits) link specific line ranges to conversation thread IDs
- The logs survive git operations: rebases, squashes, cherry-picks
- `git-ai blame` overlays AI attribution on standard git blame

**The `/ask` feature**: Talk to the agent that wrote the code about its instructions, decisions, and the engineer's intent behind the task — post-hoc conversational recall of the generation context.

Source: [Git AI: Introducing Git AI](https://usegitai.com/blog/introducing-git-ai)

### 3.6 SpecStory — Conversation History as Project Artifact

SpecStory auto-saves every Claude Code, Cursor, and Codex session as searchable markdown to `.specstory/history/` in the project directory. Key properties:
- Local-first, git-friendly (conversations can be committed, branched, merged)
- Indexed and searchable with grep/ripgrep
- Cloud sync available for cross-project search
- Converts ephemeral agent sessions into a durable, version-controlled audit trail

This is the simplest intent-preservation approach: don't capture intent explicitly, just keep the conversations that contain it.

Source: [SpecStory](https://specstory.com/)

### 3.7 Context Engineering — Codified Context Infrastructure

For large codebases, the emerging practice is "context engineering": architecting the entire information ecosystem that agents operate within.

The paper "Codified Context: Infrastructure for AI Agents in a Complex Codebase" (arXiv:2602.20478, 2026) documents a three-component infrastructure built over 283 sessions on a 108,000-line C# system:

1. **Hot-Memory Constitution**: Conventions, retrieval hooks, orchestration protocols encoded for immediate agent access
2. **19 Specialized Domain-Expert Agents**: Project-specific knowledge
3. **Cold-Memory Knowledge Base**: 34 specification documents available on demand

The key finding: persistent context infrastructure prevents agents from repeating known mistakes and maintains consistency of intent across sessions.

Source: [arXiv:2602.20478](https://arxiv.org/abs/2602.20478)

### 3.8 CodeRabbit — Intent-Aware Code Review

CodeRabbit (2025) defines "Intent" as an explicit input to code review, extracted from PR descriptions, linked Jira/Linear/GitHub issues, and learnings from prior conversations. The review system maps code changes against the underlying objectives ("context engineering, end-to-end"), checking whether what was built matches what was intended.

This closes the loop: intent captured at issue creation, used as ground truth for review of AI-generated implementation.

Source: [CodeRabbit: Context Engineering](https://www.coderabbit.ai/blog/context-engineering-ai-code-reviews)

---

## 4. The a16z Framework: Code Becoming a Compiled Artifact

Andreessen Horowitz's "Nine Emerging Developer Patterns for the AI Era" (2025) articulates the paradigm shift most clearly:

> "The source of truth may shift upstream toward prompts, data schemas, API contracts, and architectural intent. Code becomes the byproduct of those inputs — more like a compiled artifact than a manually authored source."

The proposed new primitive: **prompt+test bundles** as versionable units. Rather than tracking line-by-line diffs, version control tracks the prompt that generated the code and the tests that verify its behavior.

Git evolves from workspace to "artifact log — a place to track not just what changed, but why and by whom," with richer metadata: which agent or model made a change, where human oversight was required.

Source: [a16z: Nine Emerging Developer Patterns for the AI Era](https://a16z.com/nine-emerging-developer-patterns-for-the-ai-era/)

---

## 5. Pain Points Teams Report

Summary of documented pain points from industry reports (Qodo, CodeRabbit, Anthropic, VentureBeat, 2025–2026):

| Pain Point | Severity | Source |
|---|---|---|
| AI code has 1.7× more major bugs | High | CodeRabbit analysis, Dec 2025 |
| PR review time up 91% with high AI adoption | High | Qodo State of AI Code Quality |
| Context lost between agent sessions | Critical | VentureBeat, arXiv:2602.20478 |
| Teams can't explain why AI made a given decision | High | Tessl, AgDR, cognitive debt research |
| Senior engineers spend time on style review, not architecture | Medium | SDD research |
| Onboarding new developers to AI-generated codebases | High | Codified Context paper |
| "Context debt" from accumulating agent config files | Medium | Qodo 2026 |
| Cognitive debt: teams lose the "theory" of the system | Critical | Storey, 2026 |

---

## 6. Solution Space — What Could Be Built

### 6.1 What Exists (Today)

| Tool | Mechanism | Coverage |
|---|---|---|
| SpecStory | Save all conversations as git-committed markdown | Captures prompts, not decisions |
| Git AI | Link code lines to agent transcripts via git notes | Full lineage, but requires agent integration |
| AgDR | Structured decision records auto-generated by agents | Captures decisions, requires agent discipline |
| Archgate | ADRs as executable rules + MCP server for agents | Governance layer, not retroactive |
| Tessl / Kiro / Augment Intent | Specs as primary artifact | Prevents intent loss, requires upfront spec |
| CodeRabbit | Intent-aware review from issue tracking | Review time, not generation time |

### 6.2 Identified Gaps

1. **No automatic extraction of decisions from existing agent sessions**: Git AI requires prospective instrumentation. SpecStory saves conversations but doesn't extract decisions from them. There is no tool that takes a Claude Code session history and produces a structured decision log retrospectively.

2. **No cross-session intent continuity at the IDE level**: Agents start each session from scratch. Codified context infrastructure (hot memory) is built manually. No mainstream tool automatically builds and maintains an evolving intent model from session history.

3. **No "why" surfacing at git blame time**: `git blame` shows who/when. Git AI can show which agent. No tool surfaces the design rationale at the point of reading unfamiliar code.

4. **No intent diff on code review**: Code review tools check what changed. None check whether the change matches the intent expressed in the PR/issue description vs. the original design intent captured elsewhere (ADRs, specs).

5. **Prompt+test bundles are theorized but not implemented**: a16z identified this as the right primitive; no tool has shipped it as a first-class version control concept.

6. **No structured onboarding path**: New developers joining an AI-assisted codebase have access to SpecStory conversations and maybe some ADRs, but no synthesized "intent graph" showing what decisions were made, when, why, and which code they explain.

### 6.3 Possible Solutions

**Intent Layer Alongside Code** (CodeSpeak direction)
Treat natural-language intent as a first-class artifact in the repository. Files: `*.intent.md` or `.spec.md` beside implementation files. Agents generate code from intent; intent is version-controlled and reviewed like code. The diff on an intent file communicates design changes; the code diff is the derived output.

**Automatic Decision Extraction from Agent Conversations**
A post-processing step (run at session end or commit time) that reads the agent conversation history and extracts: decisions made, alternatives considered, constraints applied, trade-offs accepted. Outputs an AgDR-format document or appends to a running decision log. This could be a Claude Code hook: on `SessionStop`, run extraction against `~/.claude/projects/*/conversations/`.

**Intent-Augmented Git Notes**
Extend Git AI's approach: automatically attach decision summaries (not full transcripts) to commits via git notes. `git log --notes` would show: "Agent chose PostgreSQL over SQLite because of multi-user access requirement from ticket PROJ-123." This is the "why" layer in git history.

**Intent MCP Server**
An MCP server that maintains a project intent graph: specs, ADRs, AgDRs, and extracted decisions, indexed and queryable. Before writing code, agents query: "What are the existing decisions relevant to authentication?" The server returns applicable intent, preventing agents from contradicting prior decisions. Archgate's MCP server is the closest existing implementation, but scoped to ADR enforcement rather than full intent retrieval.

**Cognitive Debt Indicators in CI**
A CI check that measures the ratio of AI-generated lines to documented decisions, flags sessions with no associated AgDRs or spec updates, and reports cognitive debt accumulation over time. Analogous to code coverage metrics, but for intent coverage.

---

## 7. Key Sources

- [Andrey Breslav / CodeSpeak](https://codespeak.dev/) — foundational problem statement
- [The Pragmatic Engineer: The programming language after Kotlin](https://newsletter.pragmaticengineer.com/p/the-programming-language-after-kotlin) — Breslav interview
- [Cambridge lecture: CodeSpeak](https://www.cst.cam.ac.uk/seminars/list/242185)
- [Margaret Storey: Cognitive Debt (Feb 2026)](https://margaretstorey.com/blog/2026/02/09/cognitive-debt/)
- [Simon Willison on Cognitive Debt](https://simonwillison.net/2026/Feb/15/cognitive-debt/)
- [Tessl: Taming agents with specifications](https://tessl.io/blog/taming-agents-with-specifications-what-the-experts-say/)
- [Martin Fowler: Understanding SDD — Kiro, spec-kit, Tessl](https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html)
- [GitHub: me2resh/agent-decision-record](https://github.com/me2resh/agent-decision-record)
- [Git AI: Introducing Git AI](https://usegitai.com/blog/introducing-git-ai)
- [SpecStory](https://specstory.com/)
- [Archgate](https://archgate.dev/)
- [Augment Code: Intent](https://www.augmentcode.com/product/intent)
- [Amazon Kiro](https://kiro.dev/)
- [a16z: Nine Emerging Developer Patterns](https://a16z.com/nine-emerging-developer-patterns-for-the-ai-era/)
- [arXiv:2602.20478 — Codified Context](https://arxiv.org/abs/2602.20478)
- [Anthropic 2026 Agentic Coding Trends](https://resources.anthropic.com/2026-agentic-coding-trends-report)
- [Qodo: State of AI Code Quality 2025](https://www.qodo.ai/reports/state-of-ai-code-quality/)
- [CodeRabbit: Context Engineering](https://www.coderabbit.ai/blog/context-engineering-ai-code-reviews)
- [Knuth: Literate Programming](https://www-cs-faculty.stanford.edu/~knuth/lp.html)
- [arXiv:2106.13900 — Behavior Preservation in Refactoring](https://arxiv.org/abs/2106.13900)
- [Equal Experts: Accelerating ADRs with Generative AI](https://www.equalexperts.com/blog/our-thinking/accelerating-architectural-decision-records-adrs-with-generative-ai/)
