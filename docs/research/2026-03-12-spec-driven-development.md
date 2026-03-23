---
title: "Spec-Driven Development: Human vs Machine Specs"
date: 2026-03-12
depth: medium
agents: 1
verification_status: unverified
completion_status: complete
sources: 25+
---

# Spec-Driven Development: Human vs Machine Specs

Research commissioned by Andrey Breslav's critique of machine-generated specs. Questions: what tools exist, what is CodeSpeak, competitors, optimal spec format, spec→code roundtripping, spec diff → code diff.

---

## 1. CodeSpeak: Breslav's Position

Andrey Breslav (Kotlin creator) founded CodeSpeak in 2025. The core thesis: a spec should contain **only what the human uniquely knows** — business intent, domain-specific decisions, edge cases that require judgment. Everything the machine can infer (boilerplate, error handling, standard patterns) should be omitted.

Key principles from the Pragmatic Engineer podcast:
- Specs must be **5–10x shorter than code** (10x codebase reduction target)
- If a machine can infer it, don't write it
- Specs should be short enough to actually read and edit
- Machine-generated 3-km specs are useless — nobody reads or modifies them

**Format:** `.cs.md` Markdown files. A greenfield project spec is ~15 lines. Structure: brief project description → UX/feature bullets → technology preferences. No API schemas, no database design, no code structure.

**How it works:** CodeSpeak "compiles" specs to Python, Go, JS, TypeScript. Also does reverse: converts existing code to a minimal spec (the compression proof). Spec changes produce diff-based code updates rather than full regeneration.

**Status:** Alpha Preview as of February 2026. Hiring founding engineer.

Sources:
- https://codespeak.dev/
- https://codespeak.dev/blog/greenfield-project-tutorial-20260209
- https://newsletter.pragmaticengineer.com/p/the-programming-language-after-kotlin
- https://www.cst.cam.ac.uk/seminars/list/242185 (Cambridge CS talk)

---

## 2. The Landscape: SDD Tools in 2026

### 2.1 Kiro (AWS / Amazon) — spec-first IDE

Released mid-2025 as a VS Code fork. Three-phase workflow: **Requirements → Design → Tasks → Execution**.

- Requirements: user stories in "As a..." form + EARS acceptance criteria (WHEN X THEN Y)
- Design: architecture doc — components, data flow, schemas, error handling
- Tasks: sequenced subtasks tied back to acceptance criteria

Spec format: Markdown. Kiro can auto-generate specs from a description, then allow human editing before code generation. Agent hooks keep docs/tests in sync on file save. Supports both spec-anchored (specs evolve with code) and exploring spec-as-source (code regenerated from spec).

**Verdict on Breslav critique:** Kiro's specs are verbose by design — the three-document structure (requirements + design + tasks) is exactly the "3-km spec" Breslav objects to. The docs serve traceability and review, not minimalism.

Sources:
- https://kiro.dev/docs/specs/
- https://www.infoq.com/news/2025/08/aws-kiro-spec-driven-agent/
- https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html

### 2.2 Tessl — spec-as-source platform

The most radical tool. Specs are `.spec.md` files with three sections: component description, capabilities with linked tests, API. Generated code is marked `// GENERATED FROM SPEC - DO NOT EDIT`. The spec is the primary artifact humans maintain.

Key differentiator: **Tessl Spec Registry** — 10,000+ pre-built specs for popular OSS libraries (prevents API hallucinations). Teams publish their own specs to the registry.

**Problem noted:** Non-determinism — identical specs can produce different code outputs, requiring iterative spec refinement. The roundtrip is not clean.

Sources:
- https://tessl.io/
- https://tessl.io/blog/tessl-launches-spec-driven-framework-and-registry/
- https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html

### 2.3 GitHub Spec Kit — open-source scaffolding

72.7k GitHub stars, 110 releases through February 2026. Python CLI that creates a `specs/` folder structure with: data models, plans, tasks, research findings, API specs, components. Supports 22+ AI agent platforms (Claude Code, Copilot, Amazon Q, Gemini CLI).

**Spec evolution discussion (GitHub #152):** Community settled on: create feature specs → periodically consolidate into a master spec snapshot. No automatic sync tooling — manual discipline required. Some practitioners argue "code remains the ground truth; agents should read code, not a potentially outdated spec."

Sources:
- https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/
- https://github.com/github/spec-kit/discussions/152

### 2.4 Devin (Cognition) — autonomous agent

Does not have a spec format per se. Requires clear upfront requirements with verifiable outcomes. Key finding from 18-month performance review: **Devin performs worse when you keep telling it more after it starts**. Does not handle mid-task requirement changes. Pattern-based and migration tasks work best (inherent clarity from existing code or tooling output). Visual work needs explicit component structure, color codes, spacing.

Implication: the spec must be complete upfront. No iterative clarification. This is opposite to human-like pair programming.

Sources:
- https://cognition.ai/blog/devin-annual-performance-review-2025
- https://devin.ai/agents101

### 2.5 Sweep AI, Factory AI

Sweep AI uses GitHub issues as lightweight specs. The issue description drives the PR. No structured spec format — intent is captured in natural language issues.

Factory AI (Droids) uses a "Blueprint" concept: structured markdown with goal, context, constraints, and acceptance criteria. Similar to Kiro's requirements phase but less rigid.

### 2.6 Cursor Rules / CLAUDE.md / AGENTS.md as proto-specs

Not specs in the SDD sense, but serve a related function: persistent context for AI agents about project conventions, architecture decisions, and constraints.

- CLAUDE.md — project-level rules, loaded before every session
- `.cursor/rules/` — glob-scoped rules, closest to per-feature specs
- AGENTS.md — tool-agnostic convention gaining traction

Tool-agnostic syncing tools exist (rulesync, rule-porter). These files constrain code generation but don't describe features — more like standing architecture decisions than feature specs. They encode the "what NOT to do" half of Breslav's spec principle.

The Thoughtworks article (2025) notes that Cursor rules and similar files encode "non-functional requirements" — performance budgets, security constraints, style — that are genuinely hard for LLMs to infer.

Sources:
- https://www.agentrulegen.com/guides/cursorrules-vs-claude-md
- https://dev.to/dyoshikawatech/rulesync-published-a-tool-to-unify-management-of-rules-for-claude-code-gemini-cli-and-cursor-390f

---

## 3. Optimal Spec Format

### 3.1 What must be in a spec

Synthesizing across sources:

| Element | Why it must be explicit | Can LLM infer? |
|---|---|---|
| Business intent / domain purpose | Unique human knowledge | No |
| Ubiquitous language (DDD) | Domain-specific term definitions | No |
| Input/output contracts | Defines behavior boundaries | Partially |
| Constraints and invariants | Not derivable from "happy path" | No |
| Non-functional requirements (perf, security) | Context-specific, not universal | No |
| Edge cases that matter for THIS domain | Business-specific exception handling | No |
| Technology choices (if non-default) | Preference, not logic | No |

### 3.2 What can be omitted

- Boilerplate error handling (null checks, standard exceptions)
- Standard architectural patterns (MVC, repository pattern)
- API schema details (LLMs infer from type signatures + naming)
- Database schema internals (derivable from entity model)
- Test scaffolding (derivable from acceptance criteria)
- Logging/monitoring conventions (if encoded in CLAUDE.md/rules)

### 3.3 Length debate

**Breslav:** 5–10x shorter than code. A 1000-line module → 100–200 line spec.

**Tessl:** warns against specs that "overload the context window." Recommends one feature at a time, not full-app specs.

**Kinde (anatomy guide):** No hard length limit, but emphasizes binary/testable acceptance criteria. A good spec is "complete yet concise — covering the critical path without enumerating all cases."

**Self-Spec research (OpenReview):** Model-designed spec schemas are most compact because they match the model's internal representation. A 4-step process: schema design → instantiation → Q&A ambiguity resolution → code generation. GPT-4o: 87% → 92% pass@1 on HumanEval. The key finding: a **self-authored spec aligns with the model's internal representational bias**, reducing docstring drift.

### 3.4 DDD ubiquitous language as spec foundation

Strong convergence: multiple sources treat DDD ubiquitous language as the natural foundation for AI-age specs. Reasoning:
- Forces explicit domain vocabulary shared between humans and agents
- Prevents LLM hallucination of domain concepts ("claim" vs "ticket", "approval" vs "authorisation")
- Enables precise specs without verbose explanation — terms are pre-defined

Practical application: a glossary section in the spec (or in CLAUDE.md) that defines domain terms. Spec sentences use only these terms. The LLM fills in generic implementation; the spec fills in domain semantics.

The "Domain-Driven Agent Design" (Russ Miles, 2025) frames this as: DDD + DICE makes your prompts, ontologies, and agent actions unified under the ubiquitous language. Your agent becomes a "trusted teammate" that knows domain distinctions.

Sources:
- https://engineeringagents.substack.com/p/domain-driven-agent-design
- https://www.thoughtworks.com/en-us/insights/blog/agile-engineering-practices/spec-driven-development-unpacking-2025-new-engineering-practices

### 3.5 Gherkin/BDD assessment

**Verdict: too verbose for AI-age specs.**

Gherkin's Given/When/Then is structurally sound but has known failure modes:
- Around scenario #500, feature files become bloated and conflicting
- Non-technical stakeholders rarely write or read them in practice
- Step definition code creates extra abstraction layer

What works from BDD: the **scenario mindset** (explicit happy path + edge cases) and the concept of acceptance criteria as testable behaviors. What doesn't: the rigid 3-clause syntax, the ceremony around step definitions, the assumption of business-analyst authorship.

Current practice: SDD tools use informal Given/When/Then inside Markdown rather than `.feature` files. This captures BDD's intent without the toolchain overhead.

Sources:
- https://medium.com/@cheparsky/ai-in-testing-10-spec-driven-development-bdds-second-chance-or-just-more-docs-151e30ecc97e
- https://www.functionize.com/blog/bdd-who-needs-gherkin

---

## 4. Spec Diff → Code Diff Problem

### 4.1 Current state

This is the hardest unsolved problem in SDD. No tool has a clean, deterministic solution.

**CodeSpeak:** claims diff-based iteration — spec changes produce code diffs rather than full regeneration. This is the most promising approach but details are not public (Alpha).

**Tessl:** aspires to spec-as-source. In practice, identical specs produce non-deterministic code. Workaround: treat regeneration as a test, compare outputs.

**Kiro:** spec sync is bidirectional — you can modify code and ask Kiro to update specs, or modify spec and Kiro updates tasks. But there is no automatic code regeneration from spec diffs; a human initiates the update loop.

**GitHub Spec Kit discussion #152 (community consensus):**
1. Create feature specs, implement, then consolidate into a master snapshot
2. Master spec = current system behavior = agent's ground truth
3. Some argue: code IS the ground truth, not specs — agents should read code during a "research phase" before planning changes
4. No tool automates this; all approaches require manual discipline

### 4.2 The synchronization problem

Specs and code naturally drift during AI-driven iteration. Three strategies:

| Strategy | Mechanism | Tools |
|---|---|---|
| Spec-first, regenerate | Change spec → full regen | Tessl (partial), CodeSpeak |
| Spec-anchored | Change spec → targeted agent task | Kiro, spec-kit |
| Code-first, spec derived | Run code→spec extraction | CodeSpeak (reverse), experimental |

**Differential detection:** Some practitioners recommend "spec differential engines" — tooling that detects divergence between spec and code. No mainstream OSS tool does this today.

### 4.3 Version control for specs

Current practice: treat specs like code. They live in git alongside source. Reviewed in PRs. Diffed like any text file.

**Emerging pattern (GitHub Spec Kit):** branch per spec, then merge. This enables parallel feature development with isolated specs, then consolidation.

**Key missing tool:** A spec linter/validator that checks: (a) does the spec still describe what the code does? (b) are all spec requirements covered by tests? Only Tessl's capability-to-test linking addresses (b) partially.

---

## 5. Competitors and Adjacent Tools

| Tool | Category | Spec approach |
|---|---|---|
| CodeSpeak | Next-gen language | Minimal human spec → LLM code |
| Tessl | Agent enablement platform | Spec-as-source, registry |
| Kiro (AWS) | Agentic IDE | 3-doc structured spec |
| GitHub Spec Kit | OSS scaffolding | Multi-file spec folder |
| BMAD Method | Methodology | Story-based decomposition |
| Devin (Cognition) | Autonomous agent | No formal spec; issues/tasks |
| Factory AI (Droids) | Autonomous agent | Blueprint markdown |
| Sweep AI | PR automation | GitHub issues as specs |

**True CodeSpeak competitors** (next-gen language for AI era):
- No direct competitor found that pursues the same "compile human spec → code" vision at language level
- Tessl is the closest in aspiration (spec-as-source) but is a platform/framework, not a language
- The Pragmatic Engineer framing: "CodeSpeak occupies a middle ground between formal language and prompting" — this niche is currently uncontested

---

## 6. Open Questions and Tensions

**1. Minimal vs complete:** Breslav says 5–10x shorter than code. But Devin's performance review shows agents fail on ambiguous specs — they need explicit upfront completeness. The tension: minimal enough to maintain, complete enough for autonomous execution.

**2. Human-written vs AI-assisted:** Tessl explicitly allows "vibe-specs" (AI-generated specs) as a starting point for human editing. Self-Spec research shows model-authored schemas outperform human-written schemas. The question may not be "human OR machine" but "machine draft → human edit."

**3. Spec versioning is unsolved:** No mainstream tool provides automatic spec↔code sync. The community defaults to: specs live in git, manual discipline keeps them current.

**4. Context window limits:** Tessl warns against over-specification. Large specs degrade agent performance. This directly supports Breslav's minimalism thesis — but for a different reason (token budget, not readability).

**5. Test coupling:** Tessl and Kiro tie specs to tests. Breslav's approach is unclear here — does a minimal spec include acceptance tests or delegate that to the LLM?

---

## Sources

- https://codespeak.dev/
- https://codespeak.dev/blog/greenfield-project-tutorial-20260209
- https://newsletter.pragmaticengineer.com/p/the-programming-language-after-kotlin
- https://www.cst.cam.ac.uk/seminars/list/242185
- https://www.abreslav.com/
- https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html
- https://tessl.io/
- https://tessl.io/blog/tessl-launches-spec-driven-framework-and-registry/
- https://tessl.io/blog/spec-driven-development-10-things-you-need-to-know-about-specs/
- https://tessl.io/blog/the-most-valuable-developer-skill-in-2025-writing-code-specifications/
- https://kiro.dev/docs/specs/
- https://www.infoq.com/news/2025/08/aws-kiro-spec-driven-agent/
- https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/
- https://github.com/github/spec-kit/discussions/152
- https://cognition.ai/blog/devin-annual-performance-review-2025
- https://devin.ai/agents101
- https://openreview.net/forum?id=6pr7BUGkLp (Self-Spec paper)
- https://www.thoughtworks.com/en-us/insights/blog/agile-engineering-practices/spec-driven-development-unpacking-2025-new-engineering-practices
- https://www.kinde.com/learn/ai-for-software-engineering/best-practice/the-anatomy-of-a-good-spec-in-the-age-of-ai/
- https://engineeringagents.substack.com/p/domain-driven-agent-design
- https://www.functionize.com/blog/bdd-who-needs-gherkin
- https://www.augmentcode.com/guides/what-is-spec-driven-development
- https://developers.redhat.com/articles/2025/10/22/how-spec-driven-development-improves-ai-coding-quality
- https://blog.scottlogic.com/2025/11/26/putting-spec-kit-through-its-paces-radical-idea-or-reinvented-waterfall/
- https://ghuntley.com/specs/
