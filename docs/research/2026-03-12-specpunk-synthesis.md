---
title: "Specpunk Synthesis: AI Programming Landscape Reverse Engineering"
date: 2026-03-12
origin: Podcast Breslav & Lozhechkin (CodeSpeak discussion)
agents: 6 parallel research + 2 arbiter panel
sources: 120+
status: synthesis
---

# Specpunk Synthesis

Reverse engineering of AI coding tools landscape based on Andrey Breslav's (Kotlin creator, CodeSpeak founder) podcast discussion. Six parallel deep research agents, 120+ sources, cross-validated findings.

---

## 1. The Problem Map

Six interconnected problems extracted from the podcast, each researched independently:

```
                    ┌─────────────────────┐
                    │  Intent Evaporation  │ ← prompts discarded after code gen
                    └────────┬────────────┘
                             │
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
     ┌────────────┐  ┌─────────────┐  ┌──────────────┐
     │ Spec-Driven │  │ Code→Spec   │  │ NL Consistency│
     │ Development │  │ Conversion  │  │ Checking      │
     └──────┬─────┘  └──────┬──────┘  └──────┬───────┘
            │               │                │
            └───────────┬───┘                │
                        ▼                    ▼
              ┌─────────────────┐   ┌──────────────────┐
              │ Code Review     │   │ Next-Gen PLs &    │
              │ Bottleneck      │   │ LLM-as-Library    │
              └─────────────────┘   └──────────────────┘
```

### 1.1 Intent Evaporation (Critical)

**The core insight:** When developers use AI agents, their prompts contain intent (why, tradeoffs, constraints). After code generation, the prompt is discarded. Teammates see code, not reasoning. Breslav: "I spoke to the machine in NL; I communicate with teammates in Python."

**Quantified pain:**
- AI code has **1.7x more major bugs** (logic, race conditions, security) — CodeRabbit, 153M lines
- PR review time **+91%** with high AI adoption — Qodo 2025
- Margaret Storey (UVic) coined **cognitive debt** (Feb 2026): teams operate systems without understanding them
- Context lost between agent sessions — each starts from scratch

**What exists:**
| Tool | Mechanism | Limitation |
|------|-----------|------------|
| SpecStory | Save all conversations as git markdown | Captures prompts, not decisions |
| Git AI 1.0 | Link code lines to agent transcripts via git notes | Requires prospective instrumentation |
| AgDR | Structured decision records for agents | Requires agent discipline |
| Archgate | ADRs as executable rules + MCP server | Governance, not retroactive |
| CodeRabbit | Intent-aware review from issue tracking | Review time, not generation time |

**5 identified gaps:**
1. No automatic extraction of decisions from existing agent sessions
2. No cross-session intent continuity at IDE level
3. No "why" surfacing at `git blame` time
4. No intent diff on code review
5. Prompt+test bundles theorized (a16z) but not implemented

### 1.2 Spec-Driven Development

**Breslav's position:** specs must contain ONLY what the human uniquely knows. Machine-generated verbose specs are useless — nobody reads them.

**Three tiers** (Fowler/ThoughtWorks):
1. **Spec-first**: spec guides initial dev, then discarded
2. **Spec-anchored**: spec evolves with code
3. **Spec-as-source**: spec IS the primary artifact (CodeSpeak, Tessl)

**Tool landscape:**

| Tool | Category | Spec approach | Limitation |
|------|----------|---------------|------------|
| CodeSpeak | Next-gen language | Minimal human spec → code | Alpha, unproven at scale |
| Tessl | Agent platform | Spec-as-source, registry | Non-deterministic regeneration |
| Kiro (AWS) | Agentic IDE | 3-doc structured spec | "Sledgehammer for nuts" |
| GitHub Spec-Kit | OSS scaffolding | Multi-file spec folder | Manual discipline required |
| Devin | Autonomous agent | No formal spec | Fails on mid-task changes |

**Critical unsolved problem:** Spec diff → code diff. No tool has a clean, deterministic solution. CodeSpeak claims diff-based iteration but details not public.

**CodeSpeak's niche is uncontested:** No direct competitor pursues "compile human spec → code" at language level. Tessl is closest in aspiration but is a platform, not a language.

### 1.3 Code→Spec Conversion ("Code Takeover")

**State of art in code summarization:**
- Function-level with class skeleton context = sweet spot
- Counter-intuitive: advanced prompting (CoT) doesn't reliably beat zero-shot for summarization
- LLM summaries score 3.9-4.2 vs reference 3.0-3.5 — LLMs often BETTER than human baselines
- Repository-level requires RAG; few-shot examples are critical, not raw context

**Equivalence verification:**
- Undecidable in general (Rice's theorem)
- Practical stack: syntax → types → contracts → property-based tests → bounded proofs
- **EquiBench finding:** LLMs fail at deep semantic equivalence; excel at surface pattern matching
- **Testora** (Mar 2025): uses PR metadata as NL oracle for regression detection. 58% precision, 60% recall, $0.003/PR. Directly applicable to Code Takeover
- Testing recognizes >40% of LLM specs as sound; formal proof <4%. Testing is 10x more tractable

**Why Intermediate Representations fail (empirical):**

| Model | IR pass rate | Source code pass rate | Degradation |
|-------|-------------|----------------------|-------------|
| GPT-4 | 36% | 72% | 2x |
| LLaMA 3.1 | 18.9% | 73% | 4x |
| Code Llama | 27% | 80% | 3x |

**Three structural reasons:** loss of high-level semantics, verbosity/context explosion, control flow misinterpretation. Entity extraction as IR = information degradation. Breslav's intuition validated.

**Clover (Stanford):** The best approach — consistency triangle among code, formal annotations, and NL docstrings. 87% acceptance, 0% false positives. No IR needed.

### 1.4 NL Consistency Checking ("Type System for NL")

**What can be checked in NL specs:**

| Check | Mechanism | Difficulty |
|-------|-----------|------------|
| Undefined term reference | Glossary lookup | Low |
| Term redefinition across modules | Embedding comparison | Low-Medium |
| Synonym proliferation | Coreference resolution | Medium |
| Logical contradiction | NLI or formal logic | High |
| Missing module reference | String/ID match | Low |
| Gap / underspecification | Coverage analysis | Hard |

**Best tools found:**

| Tool | Approach | Key metric |
|------|----------|------------|
| ALICE | Formal logic + LLM | 99% accuracy, 60% recall |
| AmbiSQL | Targeted MCQ clarification | 87% F1 ambiguity detection |
| Vale | Rule-based prose linter | Deployable now, lexical only |
| Paska | Requirements smell detection | 89% precision/recall |
| GLaMoR | Graph LM for ontology consistency | 95% accuracy, 20x faster than HermiT |

**Key gap:** No production tool for automated ubiquitous language (DDD) enforcement. Has to be assembled from parts.

**Practical pipeline for spec type-checking:**
```
lexer/ → resolver/ → type_checker/ → contradiction/ → gap_detector/ → reporter/
```

### 1.5 Code Review Bottleneck

**The inversion:** Writing code is no longer the bottleneck. Reviewing it is.

**Evidence:**
- 84% devs use AI tools, only 3% "highly trust" output
- 66% cite "almost right but not quite" as primary frustration
- 45% report debugging AI code harder than debugging human code
- Code churn projected to double (GitClear, 153M lines)
- Review consumes **59.4% of all tokens** in multi-agent systems

**Counter-intuitive finding:** More detailed prompting INCREASES LLM reviewer misjudgment (arXiv:2603.00539).

**What to build (4 identified gaps):**

1. **Behavioral diff tool** — show behavioral change of a PR, not structural diff. Run PBT against old/new code. No product exists. Strong build signal.
2. **Test-based review workflow** — tests as primary review artifact, not code. Mutation score as confidence. Strong build signal.
3. **Confidence scoring for generated code** — calibrated score at PR time. Medium build signal.
4. **Spec-diff review** — diff spec against PR's NL outline. Medium build signal.

**What NOT to build:** Another AI code reviewer (market saturated), single-model LLM-as-judge (bias risk), full formal verification (10-15 year horizon).

### 1.6 Next-Gen PLs & LLM-as-Library

**Breslav's thesis: LLM = npm with NL query interface.**

Accurate for interaction model, misleading about reliability. npm packages are deterministic; LLMs are not.

**CodeSpeak compression (empirically verified):**

| Project | Compression |
|---------|-------------|
| WebVTT support (yt-dlp) | 6.7x |
| Italian SSN generator (Faker) | 7.9x |
| HTML encoding (BeautifulSoup) | 5.9x |
| EML converter (MarkItDown) | 9.9x |

**Market:** TAM $6.1B → $34.6B by 2033 (CAGR 24.2%). $9.4B VC in 2025.

**Productivity paradox:** Lines/dev up 76%, but controlled trial showed **19% net slowdown** for experienced devs. More code ≠ more value.

**What survives from traditional PLs:**
- Type systems (machine-checkable contracts)
- Module/interface boundaries
- Formal verification hooks

**What weakens:**
- Boilerplate (CodeSpeak's target)
- Dynamic typing (loses prototyping advantage)
- Language diversity (LLM corpus network effects)

**Dijkstra's objection (EWD 667, 1978) remains valid:** NL is ambiguous. LLMs paper over it with statistical regularization. Works for boilerplate, fails for novel domain logic.

---

## 2. Cross-Cutting Findings

### 2.1 The Verification Problem Is Central

Every research thread converges on the same bottleneck: **how to verify that generated code matches intent**. This is the meta-problem that subsumes all six topics.

Current best approaches:
1. **Clover triangle** (code ↔ formal spec ↔ NL docstring) — 87% acceptance, 0% false positives
2. **Testora** (NL oracle for regression) — $0.003/PR, directly applicable
3. **Property-based testing** (Cedar/AWS) — found 21 bugs that code review missed
4. **Mutation testing** as equivalence proxy — gold standard for oracle quality

### 2.2 IR Is a Trap

Multiple independent sources confirm: intermediate representations (entity extraction, ASTs, LLVM IR) make LLM performance **worse**, not better. 2-4x degradation across all models.

The correct approach: generate directly from NL, verify against behavior. Clover's triangle is the reference architecture.

### 2.3 The 80/20 Split

A consistent pattern across review, verification, and spec:
- **80% of code** is standard patterns (boilerplate, CRUD, standard error handling) — LLMs handle this well
- **20% is domain-specific** (business logic, constraints, invariants) — requires human intent
- Specs should encode the 20%; LLMs fill in the 80%
- Review effort should focus on the 20%; automation handles the 80%

### 2.4 Nobody Solved Spec ↔ Code Synchronization

Every SDD tool struggles with keeping specs and code in sync. Three strategies exist, none are clean:
1. Full regeneration from spec (Tessl, CodeSpeak) — non-determinism problem
2. Targeted agent task from spec change (Kiro) — manual initiation
3. Code-first, spec derived (experimental) — lossy

**This is the #1 unsolved problem in the space.**

---

## 3. Opportunity Map

### Tier 1: Buildable Now (tools + patterns exist, no product)

| Opportunity | Why it's open | Closest prior art | Effort |
|-------------|--------------|-------------------|--------|
| **Behavioral diff for PRs** | PBT + diff exists, no PR UX | Cedar DRT, Hypothesis | Medium |
| **Spec linter (NL type checker)** | Vale + NLI + embedding comparison | Vale, Paska, ALICE | Medium |
| **Intent extraction from sessions** | SpecStory saves, nothing extracts | AgDR format, Git AI | Small |
| **Prompt+test bundles** | a16z identified, nobody shipped | Git notes, SpecStory | Small |

### Tier 2: Hard but High-Value

| Opportunity | Why it's hard | Research base | Effort |
|-------------|--------------|---------------|--------|
| **Spec ↔ code sync engine** | Non-determinism, equivalence undecidable | Clover, Testora | Large |
| **DDD ubiquitous language enforcer** | No single tool covers pipeline | Coreference, GLaMoR, embeddings | Medium |
| **Cognitive debt dashboard** | Metrics undefined, data scattered | Storey framework, GitClear | Medium |

### Tier 3: Research-Stage

| Opportunity | Barrier | Timeline |
|-------------|---------|----------|
| Formal NL spec verification | Undecidable in general | 3-5 years |
| Deterministic spec compilation | LLM non-determinism | 2-3 years |
| Cross-session intent graph | No standard format | 1-2 years |

---

## 4. Mapping to Our Toolchain

How these findings relate to tools we already have/build:

| Finding | Our tool | Status | Action |
|---------|----------|--------|--------|
| Intent preservation in sessions | SpecStory (installed) | Captures prompts | Build extraction layer? |
| Spec-driven development | Signum pipeline | Has contracts | Extend with NL spec support? |
| Code review bottleneck | code-review skill + arbiter | Partial | Add behavioral diff? |
| NL consistency | CLAUDE.md/AGENTS.md | Manual rules | Automate with Vale rules? |
| Terminology drift | DDD glossary in specs | Not tracked | Build glossary enforcer? |
| Cross-session context | Memory bank (bank/) | Procedures/facts | Extend with intent model? |

---

## 5. Key People and Projects to Track

| Who/What | Why | URL |
|----------|-----|-----|
| Andrey Breslav / CodeSpeak | Pioneer of spec-as-source language | codespeak.dev |
| Guy Podjarny / Tessl | Spec-driven agent platform | tessl.io |
| Margaret-Anne Storey | Cognitive debt framework | UVic |
| Clover (Stanford) | Best verification architecture | arxiv:2310.17807 |
| Testora | Cheapest behavioral verification | arxiv:2503.18597 |
| ALICE (TU Berlin) | Best contradiction detector | Springer 2024 |
| AmbiSQL | Best MCQ clarification model | arxiv:2508.15276 |
| Cedar (AWS) | PBT + DRT reference case | arxiv:2407.01688 |
| MoonBit | PL designed for LLM era | moonbitlang.com |
| PromptPex (Microsoft) | Spec extraction from prompts | arxiv:2503.05070 |
| Augment Intent | Living specs with coordinator agents | augmentcode.com |

---

## 6. Breslav Assessment

**What he gets right:**
- Intent evaporation is the central problem — confirmed by all 6 research threads
- Machine-generated specs are useless — validated by Devin performance review and DDD research
- LLM-as-npm metaphor — accurate for interaction model
- IR is a trap — empirically confirmed (2-4x degradation)
- 5-10x compression — empirically verified on real OSS projects
- Module system for NL specs — the most interesting and underexplored idea

**What's unresolved:**
- Spec ↔ code sync is unsolved everywhere, including CodeSpeak
- "Gap-filling" (what LLM infers) may vary across model versions — consistency problem
- The explicit/implicit boundary is domain-dependent and hard to calibrate
- Adoption friction is real: paradigm shift to spec-as-source requires retraining
- Formal verification of NL specs remains out of reach (Dijkstra's objection holds)

**Competitive position:** CodeSpeak's niche (language-compiler-to-code from minimal NL specs) is genuinely uncontested. Tessl is the closest but is a platform. No other startup occupies this exact position.

---

## Sources Index

All 6 research files with full source citations:

1. `docs/research/2026-03-12-intent-preservation.md` — 20 sources
2. `docs/research/2026-03-12-spec-driven-development.md` — 25 sources
3. `docs/research/2026-03-12-code-to-spec-conversion.md` — 18 sources
4. `docs/research/2026-03-12-nl-consistency-checking.md` — 20 sources
5. `docs/research/2026-03-12-code-review-bottleneck.md` — 34 sources
6. `docs/research/2026-03-12-llm-as-library-nextgen-langs.md` — 15 sources

Raw research stored at: `~/vicc/docs/research/2026-03-12-*.md`
