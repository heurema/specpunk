---
title: "LLM as Code Library & Next-Generation Programming Languages"
date: 2026-03-12
tags: [research, programming-languages, llm, codespeak, spec-driven-development]
sources:
  - https://newsletter.pragmaticengineer.com/p/the-programming-language-after-kotlin
  - https://codespeak.dev/
  - https://www.cst.cam.ac.uk/seminars/list/242185
  - https://www.modular.com/blog/do-llms-eliminate-the-need-for-programming-languages
  - https://www.mjlivesey.co.uk/2025/02/01/llm-prog-lang.html
  - https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html
  - https://vivekhaldar.com/articles/llms-are-compilers/
  - https://www.moonbitlang.com/blog/ai-coding
  - https://dev.to/ionionascu/the-return-of-assembly-when-llms-no-longer-need-high-level-languages-1dak
  - https://tessl.io/blog/a-year-in-review-from-vibe-coding-to-viable-code/
  - https://www.cs.utexas.edu/~EWD/transcriptions/EWD06xx/EWD667.html
  - https://www.greptile.com/state-of-ai-coding-2025
  - https://arxiv.org/html/2503.02400v1
status: done
---

# LLM as Code Library & Next-Generation Programming Languages

## 1. Breslav's Thesis and the CodeSpeak Project

Andrey Breslav (creator of Kotlin) founded CodeSpeak in 2025. His core thesis: LLMs are best understood not as pair programmers but as enormous code repositories with a natural language query interface. The analogy to npm is precise — it is a library you call via NL instead of an API. The implication is that NL is not a conversational interface but a *query language* for stored behavioral patterns.

**CodeSpeak design:**
- A next-generation programming language that compiles to Python, Go, JavaScript, TypeScript
- Neither a formal language nor "just prompting" — occupies a middle ground
- Target audience: professional engineers on production systems, not casual users
- Core principle: "Maintain Specs, Not Code" — specs are 5-10x smaller than equivalent implementations
- In alpha preview as of early 2026 (install: `uv tool install codespeak-cli`)

**The compression claim is empirically grounded:**

| Project (open-source) | Compression |
|----------------------|-------------|
| WebVTT support (yt-dlp) | 6.7x |
| Italian SSN generator (Faker) | 7.9x |
| HTML encoding detection (BeautifulSoup) | 5.9x |
| EML converter (MarkItDown) | 9.9x |

Target: ~10x reduction in typical application code. What remains is "the essence of software engineering — only the things the human uniquely knows about what needs to happen, because everything else, the machine knows as well."

**Explicit/implicit boundary:** The human writes only what is domain-specific: business logic, constraints, unique requirements. The LLM fills in what is "obvious" — standard patterns, boilerplate, structural scaffolding. This is the gap-filling problem operationalized. The key engineering question Breslav doesn't fully resolve: how to spec what "obvious" means without creating a second informal specification layer. The Cambridge seminar (Jan 26, 2026) frames this as an open research question.

**The paradox Breslav identifies:** Today humans talk to machines in formal PL and to each other informally (Slack, docs). The inversion would be natural: talk to machines in NL, reserve formal notation for human-to-human architectural communication (types, interfaces, contracts).

---

## 2. LLM-as-Library vs. LLM-as-Pair-Programmer

The two metaphors have different engineering implications:

| Dimension | Pair-programmer metaphor | Library metaphor |
|-----------|--------------------------|------------------|
| Mental model | Conversation, negotiation | Function call, retrieval |
| Reliability expectation | Best-effort, needs review | Deterministic enough to compose |
| Failure mode | Bad advice | Wrong return value |
| How you improve output | Dialogue, more context | Better query (spec) |
| Human role | Senior reviewer | Architect defining interface |
| Composability | Low (context-dependent) | High (modular specs) |

**RAG as literal library access:** The library metaphor maps directly to Retrieval-Augmented Generation — RAG is literally fetching from a code corpus. Research shows RAG wins on "fragmented, multi-source, dialogue-heavy data" but long-context wins on structured single-source. This means RAG is the right architecture when the "library" has many small functions; long-context fits monolithic knowledge. Neither is "code search" in the classical sense — neither guarantees exact retrieval, both have hallucination risk.

**Code search vs. generation reliability:** Classical code search (grep, semantic search over ASTs) returns exact matches. LLM "retrieval" generates plausible completions that may not match any real code. This is the fundamental reliability gap. For spec-driven compilation (CodeSpeak's model), reliability comes from constrained generation against a spec rather than unconstrained retrieval — which is a meaningful engineering improvement.

---

## 3. Will Traditional PLs Die?

**Historical analogy: assembly language.** Assembly did not die — it was abstracted away. Compilers got good enough that the abstraction became invisible. The question is whether the same happens to Python/Go/Rust: not that they disappear, but that humans stop writing them directly.

Three distinct positions in the current discourse:

**Position A: PLs become intermediate representations (Ion Ionascu, dev.to).**
LLMs may collapse abstraction layers entirely — generating assembly or WASM directly from NL specs, bypassing high-level languages. If LLMs can "see" entire codebases at once, the portability argument for high-level languages weakens. PLs become documentation/verification artifacts rather than execution pathways.
*Assessment: speculative, no deployed systems demonstrate this at production scale.*

**Position B: PLs evolve, not die (Modular, MoonBit, majority view).**
Human oversight requires readable code. Code is written once but read many times. LLM-generated code still requires review — which requires readable syntax. Type systems and formal guarantees become *more* important, not less, because you need to verify generated code. Dynamic typing (Python's advantage) loses relevance when generation replaces manual authoring; static types gain value as machine-checkable contracts.

Key insight from Modular: "one of the major concerns about language models today is trust — they can give strikingly amazing results in some cases but are often subtly wrong in others." Formal verification of LLM-generated code is an active research area (arXiv 2507.13290).

**Position C: Consolidation around Python hegemony (Livesey, 2025).**
LLMs perform best with languages that have large training corpora. Research confirms LLM competency correlates with language popularity. GitHub acknowledges Copilot performs best in Python, JavaScript, TypeScript, Ruby, Go, C#. Evidence: Python grew 9.3% in 2024 vs Java (+2.3%), JavaScript (+1.4%). New languages face a bootstrapping problem — LLM support requires popularity which requires LLM support.

**What certainly survives:**
- Type systems (machine-checkable contracts for generated code)
- Module/interface boundaries (units of specification)
- Abstraction mechanisms (parameterize what the LLM fills in)
- Formal verification hooks (trusted execution paths)
- Sandboxing primitives (MoonBit's WASM focus is a response to AI-generated code security)

**What weakens:**
- Boilerplate (the explicit target of CodeSpeak's compression)
- Dynamic typing (loses prototyping advantage when LLMs prototype)
- Language diversity (network effects favor established corpora)

**Formal guarantees with NL:** Not currently achievable end-to-end. CNL-P (Controlled Natural Language for Prompts) introduces precise grammar to eliminate NL ambiguity and allow formal verification of LLM behavior. arXiv 2507.13290 proposes a Formal Query Language that represents user intent formally for verification. These approaches are early-stage research, not deployed systems.

---

## 4. The Gap-Filling Problem

When a spec compresses 5-10x, the LLM must fill in the gaps with "reasonable defaults." This creates a new class of engineering problems:

**The consistency problem.** What the LLM fills in today may differ from what it fills in tomorrow (different model version, context, temperature). A spec that worked at 6x compression with GPT-5.4 may fail differently with the next model. This is the *default behavior spec* problem: you need a second specification of what "default" means, which partially negates the compression.

**Current approaches to constraining default behavior:**
1. **Spec-as-source (Tessl):** 1:1 mapping between spec and code files, `@generate`/`@test` tags, bidirectional sync. Reduces interpretation surface area.
2. **Constitutional constraints (GitHub Spec-Kit):** Immutable architectural principles established upfront. Agents cannot violate constitutional rules.
3. **Promptware engineering (arXiv 2503.02400):** Treating prompts as first-class software artifacts with versioning, testing, formal analysis.
4. **Type-guided generation (MoonBit):** Strong type system constrains possible completions, enabling real-time static analysis during token generation.

**The calibration question (explicit/implicit boundary):** CodeSpeak's approach: humans write only domain-specific knowledge; structural patterns are implicit. The problem is that domain-specific knowledge often *depends on* structural choices — you cannot fully separate them. The Tessl/Kiro practitioners report "frequent confusion about when to stay on the functional level versus adding technical details" — this is the boundary calibration problem in practice.

**Analogy to API design:** Good API design makes the common case easy and the complex case possible. Spec design for LLM systems has the same structure — but the "common case" is filled by a probabilistic model, not a deterministic runtime. Reliability guarantees require additional layers (type checking, testing, formal verification) that partly reconstruct what was lost by moving to NL.

---

## 5. Natural Language Programming: Historical Context

**Pre-LLM NL programming attempts:**

- **COBOL (1959):** Designed to resemble English. Mostly business logic, avoided math notation. Did not eliminate programmer expertise requirement.
- **AppleScript (1993):** Readable English-like syntax. Succeeded in narrow scripting domain; never scaled to general programming.
- **Inform 7 (2006):** NL-based language for interactive fiction. Demonstrates that NL programming works in constrained domains with limited output space.
- **SQL (1974–present):** The most successful "natural language-inspired" PL — declarative, English-like, domain-specific. Works because the domain (relational queries) maps cleanly to natural language concepts.

**Dijkstra's objection (EWD 667, 1978):** Formal symbolism is not a burden — it is a tool that enables clarity impossible in natural language. Historical evidence: Greek mathematics stalled, Islamic algebra collapsed when it abandoned symbolism, modern mathematics required Descartes and Leibniz's formal notation. NL's "naturalness" makes it dangerously imprecise — it excels at "making statements the nonsense of which is not obvious." This objection remains technically valid; what has changed is that LLMs provide a probabilistic disambiguation layer that sometimes works well enough.

**The structural shift LLMs enable:** Previous NL programming attempts required the *system* to parse and interpret NL deterministically. LLMs shifted this to probabilistic interpretation with enormous training priors. This doesn't solve Dijkstra's ambiguity problem — it papers over it with statistical regularization. For most boilerplate code, the ambiguity doesn't matter because there is a clear "most likely correct interpretation." For novel domain logic, ambiguity remains fatal.

---

## 6. Prompt Programming as Paradigm

Reynolds and McDonell (2021) formalized "prompt programming" — using NL prompts as the primary interface for guiding LLM behavior. Key properties:

- Declarative intent specification rather than imperative control flow
- Few-shot examples as implicit type signatures
- Prompt structure as program structure

**Promptware engineering (arXiv 2503.02400):** Proposes treating prompts as first-class software with engineering practices — versioning, testing, formal analysis. Identifies class of bugs unique to promptware: sensitivity to phrasing, inconsistency across runs, context-length-dependent behavior.

**The NL-as-programming-language claim:** If prompts are programs, NL is a programming language. The properties it has that traditional PLs lack: enormous expressivity, ambiguity tolerance, cultural/domain knowledge encoded in weights. Properties it lacks: determinism, formal semantics, compositional guarantees, verifiability.

**CodeSpeak's position:** NL alone is not sufficient — you need modularity and reuse mechanisms to let humans organize code and collaborate. This is why CodeSpeak is more like Python than like Kotlin: it adds engineering structure *around* NL rather than formalizing NL itself.

---

## 7. Spec-Driven Development (SDD) Landscape

SDD emerged in 2025 as a formal practice distinct from "vibe coding." Key milestone: AWS launched Kiro (July 2025), GitHub released Spec-Kit (October 2025).

**Three tools analyzed (Fowler, martinfowler.com):**

**Kiro (AWS, simplest):** Spec-first workflow — Requirements (user stories with acceptance criteria) → Design → Tasks. Each phase is a markdown document. Memory bank with product.md, tech.md, structure.md. Critique: "like using a sledgehammer to crack a nut" for small bug fixes — overhead doesn't scale down.

**Spec-Kit (GitHub, CLI-based):** Constitutional foundation + extensive markdown artifacts + slash command integration with Copilot, Claude Code, Gemini CLI. Specs become "shared source of truth." Aspires to spec-anchored development but practices spec-first in reality.

**Tessl (most ambitious):** Only tool explicitly targeting spec-as-source level. 1:1 spec-to-code file mapping, `@generate`/`@test` tags, exploring bidirectional sync. Lowest interpretation surface — reduces what LLM must infer. Critique: non-determinism creates control illusions despite elaborate workflows.

**Unresolved tensions across all SDD tools:**
- Problem size applicability (overhead kills small tasks)
- Target user undefined (solo dev vs. product teams)
- Spec maintenance strategy over time
- False sense of control when agents over-interpret or ignore specs

**Tessl's 2025 diagnosis:** "From vibe coding to viable code" — the pivot point was Jason Lemkin's Replit production incident (July 2025) where an AI agent ignored a code freeze, fabricated data, and deleted a production database. SDD emerged as the structural response.

---

## 8. Market Landscape

**Market size:** Global AI Coding Startup Platforms Market valued at $6.1B in 2025, projected $34.6B by 2033, CAGR 24.2%. In 2025 alone, AI developer platforms attracted $9.4B in venture funding.

**Adoption:** ~84% of developers using or planning to use AI coding tools. GitHub Copilot holds 42% of engineering managers' preference; Cursor surged to ~40% of AI-assisted PR market by October 2025.

**Developer sentiment shift:** Favorable views toward AI tools dropped from >70% (2023-2024) to 60% (2025). Root cause: randomized controlled trial showed AI tools caused 19% net *slowdown* among seasoned open-source contributors. Hidden taxes: cognitive load of context switching, verification burden, subtle defects (race conditions, security vulnerabilities).

**Productivity paradox:** Lines of code/developer up 76% (4,450 → 7,839). But more code is not more value — denser, harder-to-review code. PR size up 33% median (57 → 76 lines). Quality metrics unclear.

**Why AI coding startups pivot:**
1. *Chasing hype cycle:* "Vibe coding" positioned as accessible to non-engineers. Reality: production systems require engineering discipline regardless.
2. *Wrong TAM framing:* TAM is "everyone who might write code" but the real market is "professional engineers who need to maintain production systems." These are different tools.
3. *Copilot captured the commodity layer:* Tab completion / inline suggestions are commoditized. Differentiation requires moving up (spec-level) or down (specialized domain).
4. *Enterprise caution:* Most large enterprises still in proof-of-concept phase, haven't made long-term tool commitments.

**LLM consolidation risk for PLs:** LLM competency correlates with language training corpus size. This creates moats around Python, JavaScript, TypeScript. New languages face a bootstrapping trap: poor LLM support → slow adoption → small corpus → poor LLM support. MoonBit is attempting to escape this by designing explicitly for LLM-assisted development (strong types, built-in testing, WASM target) rather than human ergonomics.

**What a "next-gen tools for engineers" TAM actually looks like:**
- Professional software engineers: ~27M globally (2025 estimate)
- Average tooling spend: $3-5K/engineer/year
- Addressable: ~$80-130B (broad) vs ~$20-30B (AI-specific tooling)
- Realistic near-term: $6-10B by 2028 for AI coding tools specifically (based on current trajectory)

---

## 9. Open Questions and Synthesis

**The strongest argument for NL-as-PL succeeding:** LLMs are already the primary query interface for "what does X library do" and "write me a function that does Y." The mental model has already shifted — prompts are programs, people already think this way. CodeSpeak is formalizing an existing practice.

**The strongest argument against:** Dijkstra's problem remains. NL is ambiguous. Compression comes at the cost of verifiability. Every 6-10x spec reduction requires a 6-10x increase in trust that the LLM's interpretation is correct. That trust requires testing infrastructure, type systems, formal verification — which are themselves programming artifacts. You cannot eliminate the complexity, only relocate it.

**The assembly analogy's limit:** Assembly didn't disappear because we built better compilers. But we also built languages *designed* for compiler translation — formal, deterministic, unambiguous. LLMs are not deterministic compilers. The correct analogy may be: LLMs are what a compiler would be if it had a fuzzy parser and sometimes generated plausible-but-wrong output. You would not ship to production without verification layers.

**Net assessment on Breslav's thesis:**
- "LLM as npm" — accurate for the interaction model, misleading about reliability. npm packages are deterministic; LLMs are not.
- "NL is the only query language" — accurate descriptively (that's how people use LLMs), normatively premature (formal specs still outperform NL for reliability).
- "CodeSpeak more like Python than Kotlin" — the modularity/reuse claim is the most interesting and underexplored part. Can you have a module system for NL specs that composes without interaction effects? Open research question.
- "PLs won't die but evolve to higher level" — well-supported by evidence. The direction is: types and interfaces survive, syntax sugar and boilerplate die, dynamic languages lose their prototyping advantage.

**The inversion Breslav identifies is real but partial:** Humans *should* communicate with machines in NL and with each other in formal notation (types, contracts, interfaces). This is already the direction spec-driven development is moving. But the transition requires solving the verification problem at the NL→code boundary — which is not solved.

---

## Sources

- Pragmatic Engineer: "The programming language after Kotlin" — https://newsletter.pragmaticengineer.com/p/the-programming-language-after-kotlin
- CodeSpeak official site — https://codespeak.dev/
- Cambridge CST seminar (Breslav, Jan 2026) — https://www.cst.cam.ac.uk/seminars/list/242185
- Modular: "Do LLMs eliminate the need for programming languages?" — https://www.modular.com/blog/do-llms-eliminate-the-need-for-programming-languages
- M J Livesey: "Do LLMs spell the end for programming language innovation?" — https://www.mjlivesey.co.uk/2025/02/01/llm-prog-lang.html
- Martin Fowler: "Understanding Spec-Driven-Development: Kiro, spec-kit, and Tessl" — https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html
- Vivek Haldar: "LLMs are compilers" — https://vivekhaldar.com/articles/llms-are-compilers/
- MoonBit: "The future of programming languages in the era of LLM" — https://www.moonbitlang.com/blog/ai-coding
- Ion Ionascu: "The Return of Assembly: When LLMs No Longer Need High-Level Languages" — https://dev.to/ionionascu/the-return-of-assembly-when-llms-no-longer-need-high-level-languages-1dak
- Tessl: "2025 Year in Review: From Vibe Coding to Viable Code" — https://tessl.io/blog/a-year-in-review-from-vibe-coding-to-viable-code/
- Dijkstra EWD 667: "On the Foolishness of Natural Language Programming" (1978) — https://www.cs.utexas.edu/~EWD/transcriptions/EWD06xx/EWD667.html
- Greptile: "State of AI Coding 2025" — https://www.greptile.com/state-of-ai-coding-2025
- arXiv 2503.02400: "Promptware Engineering" — https://arxiv.org/html/2503.02400v1
- Congruence Market Insights: AI Coding Startup Platforms Market Report — https://www.congruencemarketinsights.com/report/ai-coding-startup-platforms-market
- GitHub Spec-Kit — https://github.blog/ai-and-ml/generative-ai/spec-driven-development-with-ai-get-started-with-a-new-open-source-toolkit/
- Tessl Framework — https://tessl.io/blog/tessl-launches-spec-driven-framework-and-registry/
