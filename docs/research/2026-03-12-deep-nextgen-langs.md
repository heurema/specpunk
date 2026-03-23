---
title: "Next-Gen Programming Languages & LLM-as-Library: Deep Dive (Second Pass)"
date: 2026-03-12
tags: [research, programming-languages, llm, spec-driven-development, deep-research]
status: done
---

# Next-Gen Programming Languages & LLM-as-Library: Deep Dive

**This is the second-pass document. It does not repeat findings from `2026-03-12-llm-as-library-nextgen-langs.md`.**
**What is NOT here:** CodeSpeak data, LLM-as-npm metaphor, PL death positions, gap-filling, Dijkstra EWD 667, market TAM, MoonBit overview, Modular overview, promptware arXiv 2503.02400, SDD tool landscape (Kiro/Spec-Kit/Tessl high-level).

---

## 1. Other "Next-Gen" Language Projects

### 1.1 Mojo: The AI-Era Systems Language

Mojo (Modular) is the most credible attempt to build a new general-purpose language explicitly positioned for the AI era. Unlike CodeSpeak (which targets spec-to-code compilation), Mojo targets the *generation target* layer — what LLMs should output when writing high-performance AI code.

**Technical positioning:** Mojo is designed as a strict superset of Python, with MLIR as its compilation substrate. This is architecturally significant: MLIR (Multi-Level Intermediate Representation) is itself a framework for building compilers, not a fixed IR. Mojo can compile to any MLIR target, including custom accelerators — a capability Python with CUDA cannot match without FFI.

**1.0 roadmap (H1 2026) key additions:**
- **Compile-time reflection:** Structs can be introspected at compile time, enabling automatic trait conformance (equatability, JSON serialization, CLI parsing) without boilerplate macros
- **Linear types:** Values with explicit destruction semantics. This is a direct response to memory safety concerns in AI-generated code — the type system can enforce that certain objects (e.g., GPU tensors) are not duplicated or forgotten
- **Typed errors:** Functions declare the exact type they raise. This eliminates the `catch-all Exception` antipattern common in Python and makes LLM-generated error handling more verifiable

**LLM code generation for Mojo specifically:** Existing LLMs struggle with Mojo. MojoBench (arXiv 2410.17736, NAACL 2025) created HumanEval-Mojo — the first benchmark for Mojo code generation — and found that top models (GPT-4o, Claude 3.5 Sonnet) underperform significantly on Mojo vs Python. Mojo-Coder, a specialized fine-tuned model, achieves 30-35% improvement over GPT-4o on Mojo tasks. The bootstrapping problem (new language → poor LLM support → slow adoption → poor LLM support) is real and empirically measured.

**Modular's explicit position on LLMs and PLs:** "Programming languages serve three critical functions: human-to-computer communication, human-to-human communication, and computer-to-human output (readable generated code). As LLMs generate code, readability becomes even more critical — developers must review and validate AI-generated output." (Modular blog, "Do LLMs Eliminate the Need for Programming Languages?") This is a deliberate counter-positioning: PLs are not being eliminated, they are becoming verification and review interfaces.

**Modular 26.1 milestone:** MAX Python API graduates from experimental — production-ready with PyTorch-like eager mode and `model.compile()`. Mojo gains compile-time reflection, linear types, typed errors. As of early 2026, >450K lines of code, 6000+ contributors.

### 1.2 DSPy: Prompt Programming as Language Design

DSPy (Stanford NLP, "Declarative Self-improving Language Programs in Python") is not technically a new language, but it implements a programming language paradigm over LLMs. It deserves analysis as a language design artifact.

**Core language design choices:**
- **Signatures as type declarations:** A DSPy signature is essentially a type annotation — `"question: str -> answer: str"` declares the input/output contract of an LLM module. This is the closest thing to a type system for NL programs
- **Modules as first-class composable units:** ChainOfThought, Predict, Retrieve are parameterized modules that implement different "calling conventions" for the underlying LLM
- **Optimizers as compilers:** DSPy optimizers (BootstrapFewShot, MIPROv2, GEPA) take a program + few examples + metric and produce an "optimized" version by searching the prompt space. This is compilation: transforming a high-level specification into a lower-level executable form

**GEPA (Genetic Pareto Optimization, July 2025):** The most significant DSPy advance in 2025. GEPA writes prompts that often outperform the best human-engineered prompts while being highly sample-efficient. It treats prompt optimization as a multi-objective search problem (accuracy + efficiency on the Pareto frontier). This is evolutionary compilation of NL programs — a concept without precedent in traditional PL theory.

**What DSPy proves for the LLM-as-library thesis:**
1. Composable NL modules *are* feasible — DSPy programs compose without interaction effects when signatures are well-defined
2. The "interface contract" in NL is the signature — input/output field types enforced by the runtime
3. Version stability is partially achievable — optimized programs are stored as artifacts and can be reproduced (subject to LLM non-determinism)
4. The analogy breaks at reliability: DSPy modules are not deterministic and require empirical evaluation to validate composition correctness

**ICLR 2024 paper results:** DSPy programs outperformed manually engineered prompts on all tested tasks (HotpotQA: +13.5%, HotpotQA retrieval: +7.3%, MATH: +6.1%) while using 3-5x fewer labeled examples.

### 1.3 AI-Oriented Grammar: SimPy and DualCode

A 2024 paper (arXiv 2404.16333, ISSTA 2024) introduced a conceptually important idea: the grammar of programming languages should be optimized not just for human readability but for LLM generation efficiency.

**SimPy:** An AI-oriented grammar for Python that eliminates formatting tokens and minimizes redundancy while preserving AST structure. Results:
- 13.5% token reduction (CodeLlama)
- 10.4% token reduction (GPT-4)
- Maintained or improved task performance despite fewer tokens

**DualCode framework:** Bidirectional converter between SimPy (AI-facing) and Python (human-facing). Users interact with Python; the model generates SimPy internally; the runtime converts outputs back. This is a dual-representation architecture — the first practical implementation of "same program, multiple syntactic views."

**Implication for language design:** This challenges the foundational assumption that PLs must optimize for human readability. In an era where LLMs generate most code, the human-facing and AI-facing grammars may legitimately diverge. Future languages may have two canonical forms: one for human review, one for AI generation.

**Related work: SynCode (arXiv 2403.01632):** Grammar-constrained LLM generation using automata-based constraints. Instead of post-hoc filtering, SynCode guides generation token-by-token to only produce grammatically valid code. This eliminates syntax errors entirely — zero syntax-invalid outputs. Reduces compilation errors; increases functional correctness on code synthesis tasks (published: PLDI 2025 adjacent, broad adoption in constrained generation).

### 1.4 Roc, Hazel, Gleam: The Research/Functional Frontier

Three projects represent different vectors of PL research that intersect with the LLM era:

**Roc (Richard Feldman):** First numbered release (0.1.0) expected 2026, after a full compiler rewrite targeting Advent of Code 2025 usability. Design goals: purely functional, no runtime exceptions, fast compilation, strong type inference. The new compiler architecture (not yet public) is designed for incremental compilation — critical for LLM-assisted workflows where you want instant feedback on generated code. No explicit LLM-first design claims, but the strong type system + no-exception semantics would make generated code more verifiable.

**Hazel (Cyrus Omar, U Michigan):** Live functional programming environment with typed holes — the ability to typecheck and run *incomplete* programs. PLDI/POPL-adjacent, multiple papers in 2025:
- "Incremental Bidirectional Typing via Order Maintenance" (OOPSLA 2025, Distinguished Paper): Incrementalizing type checking — critical for real-time feedback during LLM generation
- "Syntactic Completions with Material Obligations" (OOPSLA 2025): Systematic syntax error correction by repair — automatic fix proposals for malformed LLM output
- "Grove: A Bidirectionally Typed Structure Editor Calculus" (POPL 2025): Formal foundations for collaborative editing with commutative edit actions

Hazel is most directly relevant to the LLM-assisted development future: if your IDE can typecheck and run half-generated programs, you can interactively steer LLM generation with immediate feedback rather than end-to-end generation cycles.

**Gleam (v1.x, 2024-2025):** BEAM-targeting functional language with static typing. Stack Overflow Developer Survey 2025: 2nd most admired language (70% of users want to continue). An AI SDK for Gleam already exists providing unified LLM provider interfaces. Gleam's significance for the LLM era: it demonstrates that a new language can achieve adoption *despite* poor initial LLM support if the developer experience is good enough. LLM support follows adoption, not the other way around — but Gleam's trajectory is slow.

### 1.5 AIOS Compiler: LLM as Interpreter

arXiv 2405.06907 ("AIOS Compiler: LLM as Interpreter for Natural Language Programming and Flow Programming of AI Agents") proposes a unified representation for three programming paradigms:
1. Natural language programs (free-form instructions)
2. Pseudo-code programs (structured but informal)
3. Flow programs (explicit DAG-based control flow)

The LLM serves as the interpreter across all three. Key architectural decisions:
- **Structured syntax:** Even NL programs have defined syntax structure — not free-form text but organized instruction sequences
- **External memory:** Reduces redundancy; the interpreter doesn't re-interpret the same instructions at each step
- **Tool invocation:** LLM compensates for its own limitations by calling external tools for precision tasks

**What this demonstrates:** The LLM-as-interpreter model collapses the compile/run distinction that defines traditional PLs. There is no bytecode; there is no fixed execution model. The same NL specification can be "executed" differently depending on the LLM, its temperature, its context. This is a feature (flexibility) and a bug (non-reproducibility). The AIOS architecture is available open-source (CoRE, OpenAGI, AIOS repositories).

---

## 2. LLM-as-Library: Detailed Analysis

### 2.1 npm/PyPI Analogy: Exact Parallels and Breaks

The npm/PyPI analogy is useful but breaks in specific, measurable ways:

| Dimension | npm/PyPI | LLM as library |
|-----------|----------|----------------|
| Installation | Explicit version pin | Model version implicit in API endpoint |
| Reproducibility | Deterministic (same version → same bytes) | Non-deterministic (same prompt → different tokens) |
| Interface | Typed function signature | NL signature (informal, interpreted) |
| Versioning | Semantic versioning (semver) | Model capability versioning (no standard) |
| Caching | Filesystem cache (100% hit rate for same version) | Semantic cache (~31% of queries are repeated) |
| Testing | Unit tests against fixed behavior | Property-based tests against statistical behavior |
| Side effects | Explicit (I/O, network) | Implicit (training data biases, instruction following) |
| License | Explicit (MIT, Apache, GPL) | Model terms of service (often restrictive) |
| Breaking changes | Semver MAJOR bump | Silent (model updates mid-deployment) |
| Tree shaking | Dead code elimination | No equivalent — entire model always loaded |

**The critical break:** npm packages are content-addressed; given a package name + version, you get identical bytes. LLM outputs are not content-addressed even with deterministic settings. This means the LLM-as-library analogy fails at the point where library reliability matters most: reproducible builds and regression testing.

**Where the analogy holds precisely:** The *interaction model*. You express intent in a query language (NL), the library returns a result, you compose results. This is functional programming — referentially transparent in intent even if not in execution. The analogy works for thinking about architecture but breaks for thinking about reliability.

### 2.2 Has Anyone Built a Package Manager for LLM Capabilities?

**Tessl Spec Registry (Sept 2025, open beta):** The closest implementation to date. 10,000+ "Usage Specs" — structured NL specifications of OSS library APIs, versioned to match library versions. Distributed as packages. `npm install @tessl/react-19` installs a spec describing React 19's API — agents use this instead of hallucinating APIs from stale training data.

This addresses a different problem than LLM capability packaging — it packages *knowledge about existing libraries* rather than LLM capabilities themselves. But the infrastructure is identical to a package manager: versioned artifacts, dependency resolution, registry, install command.

**Why no one has built a true LLM capability package manager:**
1. LLM capabilities are not discrete or compositional in the way library functions are
2. No formal interface language exists for LLM capabilities — there is no type system for "can generate valid SQL" or "can parse medical records"
3. Capabilities degrade gracefully rather than failing with errors — there is no clean interface contract to verify against
4. Model providers change model behavior without explicit versioning — any capability package would immediately drift

**MCP (Model Context Protocol) as partial answer:** Anthropic's MCP provides a standardized interface for *tool invocation* — but tools are deterministic external functions, not LLM capabilities. MCP is closer to a plugin API than a capability package manager.

### 2.3 Deterministic LLM Research: Making Output Reproducible

**The fundamental problem:** Even temperature=0 does not guarantee identical outputs. Root cause: floating-point arithmetic non-associativity in GPU matrix operations. Different batch sizes cause different kernel execution orders, cascading into measurable output drift. This is called *batch invariance failure*.

**Thinking Machines Lab breakthrough (2025):** Built batch-invariant kernels for all three non-associative operations (RMSNorm, matrix multiplication, attention) and integrated them into vLLM. Result: 1,000 identical prompts → 1,000 identical outputs. Perfect reproducibility demonstrated.

**The performance cost:** Deterministic inference is ~62% slower (26s → 42s for 1,000 sequences). Hardware requirement: NVIDIA GPUs with compute capability 9.0+ (H100/Hopper generation or newer).

**vLLM's approach:**
- Offline mode: `VLLM_ENABLE_V1_MULTIPROCESSING=0` makes scheduling deterministic
- V1 default: `seed=0` ensures consistent random state per worker
- Batch invariance: explicit feature flag, documented performance penalty

**Implications for LLM-as-library:** If same spec → same output, the caching model works. A semantic cache with 100% hit rate for identical specs *would* make LLMs library-like. But this requires:
1. Deterministic inference (62% slower, H100 hardware)
2. Semantic equivalence detection to recognize "same spec" expressed differently (vCache research shows this grey zone is unresolved)
3. Controlled model versions (API providers update models without announcement)

This combination is achievable in a controlled on-premises deployment but not via commercial APIs. The library metaphor holds for private deployments with pinned models; it fails for cloud API consumption.

### 2.4 Semantic Caching: Toward a Library of Outputs

**How semantic caching works:** Queries → vector embeddings (768-1536 dims) → cosine similarity. Cache hit if similarity > threshold (0.85-0.95). MeanCache (2024): detects semantically similar queries and reuses cached answers. Academic studies: ~31% of real LLM queries are exact or semantic repeats.

**GPTCache (Zilliz):** Production semantic cache, integrated with LangChain and LlamaIndex. Stores embeddings of queries and responses; on cache hit, skips LLM call entirely. Latency reduction: 10-100x for cached queries. Cost reduction: proportional to cache hit rate.

**The grey zone problem (vCache, 2025):** Similarity distributions for correct and incorrect hits overlap heavily. Embedding geometry alone cannot reliably separate "same intent expressed differently" from "different intent, similar phrasing." LLM-as-judge is needed for high-stakes equivalence decisions — but this adds LLM calls back into the hot path.

**Semantic caching as proto-library:** If you define a "library function" as a mapping from specification → implementation, semantic caching approximates this: same spec → cached implementation (exact) or similar spec → semantically equivalent implementation (approximate). The approximation error is the cache's false positive rate (estimated 5-15% for typical thresholds).

**Practical implication for spec systems:** If CodeSpeak or Tessl can normalize specs to a canonical form (eliminating equivalent phrasings), exact-match caching becomes feasible. This is the direction that makes the library metaphor technically viable.

### 2.5 Retrieval vs. Generation: When to Retrieve vs. Generate

Research on retrieval-augmented code generation (arXiv 2503.20589, 2510.04905) provides an empirical decision framework:

**Retrieve when:**
- Task is repository-level (long-range dependencies exist in the repo)
- API documentation is available and semantically matchable
- The codebase context is more informative than training data
- Retrieved content is structurally relevant (not just syntactically similar)

**Generate when:**
- No high-quality retrieval corpus exists
- Task requires novel logic without existing analogues
- Retrieved similar code introduces noise (up to 15% performance degradation from noisy retrieval)
- The query is for well-known patterns that LLM training already covers thoroughly

**AllianceCoder (arXiv 2503.20589) decision framework:**
1. Decompose query via chain-of-thought into implementation steps
2. Retrieve APIs via semantic description matching (not code similarity)
3. Retrieve repository context for structural dependencies
4. Generate code conditioned on retrieved structured knowledge

Results: Pass@1 improvement up to 20% on CoderExec and RepoExec benchmarks vs. pure generation.

**The key insight:** Code similarity retrieval hurts because syntactically similar code has different semantic context. API documentation retrieval helps because it provides precise behavioral contracts. This maps to the spec-driven development insight: the spec is more like API documentation than like code — it describes behavior, not implementation.

---

## 3. Module Systems for NL Specifications

### 3.1 DSPy: The Only Implemented NL Module System

DSPy is the most concrete implementation of composable NL modules with explicit interface semantics. Key module system properties:

**Signature as interface:** `"context: str, question: str -> answer: str, confidence: float"` — this is a typed interface contract in NL. The DSPy runtime enforces that inputs/outputs match declared types. Type mismatches raise Python errors, not subtle prompt failures.

**Module composition mechanics:**
```python
class RAGPipeline(dspy.Module):
    def __init__(self):
        self.retrieve = dspy.Retrieve(k=3)
        self.generate = dspy.ChainOfThought("context, question -> answer")

    def forward(self, question):
        context = self.retrieve(question).passages
        return self.generate(context=context, question=question)
```

This composes without explicit interface negotiation — the `context` field in `generate`'s signature matches the output of `retrieve`. The module system works because field names are the interface contract.

**Joint optimization of composed modules:** MIPRO and BootstrapFewShot can optimize the entire pipeline jointly — not just individual modules. This is the equivalent of whole-program optimization in traditional compilers: cross-module interactions can be optimized based on end-to-end metrics.

**What makes this work:** The composed modules share a runtime (DSPy's LM calls), so the optimizer can observe cross-module behavior. This is fundamentally different from traditional library composition where modules execute independently.

### 3.2 Tessl's Spec Package System: Dependency Management for NL Specs

Tessl's Spec Registry (September 2025) implements what is arguably the first package manager for NL specifications:
- Package format: `.spec.md` files with structured YAML frontmatter
- Versioning: matched to the library being described (e.g., `@tessl/react-19` corresponds to React v19.x)
- Dependency resolution: standard package manager semantics
- Distribution: public registry + private organizational registries
- Consumption: `install @tessl/<package>` command; local installation for agent access

**What composition looks like:** Agent working on a React 19 + Prisma 6 project installs both spec packages. The specs coexist without conflict because they describe different API surfaces. The agent can reference both simultaneously without namespace collisions — because the specs are about different libraries.

**The namespace collision problem for general NL modules:** When two NL modules define "user" differently (e.g., one is about OAuth users, one is about database users), there is no formal resolution mechanism. Tessl sidesteps this because its specs are library-specific, not domain-specific. A general NL module system would need:
1. Explicit namespace declarations: `module auth.user` vs `module db.user`
2. Qualified names in specs: reference `auth.user.id` not just `user.id`
3. Import disambiguation: `import { User as AuthUser } from auth`

None of these mechanisms exist in current NL spec systems. The problem is isomorphic to namespace collision in code — but harder because NL terms lack the syntactic distinctiveness that makes module qualification in code unambiguous.

### 3.3 Interface Contracts Between NL Modules: What Verification Looks Like

**The NL-to-LTL pipeline as contract verification infrastructure:**

Recent work (arXiv 2512.17334) demonstrates automated NL→LTL translation at 88.4% semantic accuracy, 100% syntactic correctness on aerospace requirements. The Req2LTL approach uses OnionL as an intermediate representation:

```
NL Requirement → OnionL (hierarchical IR) → LTL formula
"The engine shall remain off until the key is turned" →
    Scope(Global) + Relation(Until) + AP(engine.off) + AP(key.turned) →
    G(¬key.turned → ¬engine.on) ∧ (key.turned ↔ engine.on)
```

This is formal contract verification at the NL level. Applied to NL module interfaces:
- Each module's behavioral contract can be expressed as LTL
- Contract compatibility can be formally verified: does module A's outputs satisfy module B's preconditions?
- Composition verification: does the composed pipeline satisfy the end-to-end LTL contract?

**Current accuracy limits:** 88.4% on aerospace requirements (simple temporal logic). Complex business logic with deeply nested conditions (63.2% of industrial requirements) and implicit temporal cues remains challenging. This means ~12% of interface contracts would be incorrectly formalized — too high for safety-critical use, marginal for business applications.

**ConformalNL2LTL (arXiv 2504.21022, Feb 2026):** Achieves user-defined translation success rates through iterative question-answering. The system can guarantee (with conformal prediction) that translation accuracy meets a user-specified threshold. This is the first NL→formal specification system with statistical correctness guarantees.

### 3.4 Import/Export Semantics: What Would "import auth module" Mean?

In current code: `import { OAuth } from './auth'` brings a deterministic, versioned artifact into scope. The compiler verifies that `OAuth` is exported from `auth`, that it has the expected type, and that all usage sites match.

For NL modules, equivalent semantics would require:
1. **Discovery:** The runtime locates the spec for `auth` — either from a registry (Tessl model) or a local file
2. **Context injection:** The auth spec is prepended to the LLM context when generating code in the current module — making auth's semantics available to the generator
3. **Boundary enforcement:** Any code touching auth-domain concepts is regenerated within the auth spec's context, preventing context drift
4. **Contract checking:** Post-generation, LTL formulas derived from both modules are checked for mutual satisfaction

**What "export" means for NL modules:** A module exports a behavioral contract — what it guarantees, not how it implements it. In LTL: `G(request → eventually response)`. Consumers import this guarantee; the internal implementation can be regenerated without breaking consumers if the contract is maintained.

**The dependency version constraint problem:** In semver, `^1.2.0` means "compatible with 1.2.0". For NL spec modules, version compatibility means: behavioral contracts are still satisfied. But LLM interpretation of a slightly changed spec may differ unpredictably. This is harder than semver: you cannot detect breaking changes by version numbers, only by behavioral testing.

---

## 4. The Compilation Model

### 4.1 What "Compiling" NL to Code Actually Means

**Formal definition attempts:** No consensus definition exists, but three positions in the literature:

**Position 1 (AIOS/CoRE):** LLM *is* the compiler. NL is input, code is output, the model's weights are the compilation rules. No separate compilation phase — interpretation happens at runtime.

**Position 2 (Tessl/SDD):** Compilation is a structured workflow: requirements → design → tasks → code. Each phase is a separate LLM invocation with constrained output format. The "compiled artifact" is the task breakdown + generated code, both validated against the spec.

**Position 3 (Req2LTL/formal):** Real compilation requires a formal intermediate representation. NL → formal IR → code. The IR (e.g., OnionL, abstract syntax tree of intent) allows verification, optimization, and cross-compilation. Without an IR, you have interpretation, not compilation.

**The most defensible definition:** NL compilation is the translation of an NL specification through one or more intermediate representations into executable code, where each translation step is verifiable against a formal contract. By this definition, DSPy is closest to a compiler (signatures as IR), Tessl aspires to be a compiler (specs as source, `@generate` as compilation directive), and vibe coding is an interpreter (no intermediate representation, no verification).

### 4.2 Multi-Pass Compilation: NL → Abstract Spec → Concrete Spec → Code

The arXiv 2602.00180 paper formalizes a three-level spec hierarchy:

1. **Spec-First level:** High-level intent (user stories, acceptance criteria). Abstract, behavioral. Example: "Users can reset their password via email."
2. **Spec-Anchored level:** Technical constraints, architectural decisions, data models. Concrete enough to generate. Example: "Password reset: POST /auth/reset-request, JWT token, 15min TTL, bcrypt hash."
3. **Spec-as-Source level:** 1:1 mapping to code files. Generation directive: `@generate`. Example: Tessl's `.spec.md` files that generate exactly one implementation file.

**Multi-pass compilation flow:**
```
User intent (free NL)
    ↓ [requirements agent]
Structured requirements (EARS notation, user stories + ACs)
    ↓ [design agent]
Technical design (API contracts, data models, architecture)
    ↓ [task decomposition agent]
Implementation tasks (atomic, verifiable units)
    ↓ [code generation agent]
Source code (verified against spec)
```

Kiro implements this 3-phase flow (requirements.md → design.md → tasks.md → code). Tessl aspires to a 1-phase flow (spec.md → code), compressing the middle passes.

**Why multi-pass matters for reliability:** Each pass reduces the interpretation surface area. Going directly from user intent to code requires enormous inference — the LLM must simultaneously reason about requirements, design, architecture, and implementation. Each intermediate representation constrains what the next agent must infer.

**Verification at each pass:** The arXiv 2602.00180 paper cites "50% error reduction" when human-reviewed specs are used vs. auto-generated. This is the cost of multi-pass: human review at each boundary. The tradeoff is quality vs. latency.

### 4.3 Can You Optimize a Spec?

**Optimization passes in traditional compilers:** Dead code elimination, constant folding, loop unrolling, inlining. Each eliminates redundancy or resolves ambiguity.

**Spec optimization analogues:**
- **Redundancy elimination:** "Users can log in with email/password OR Google OAuth" + "All authentication requires MFA" → "Users can log in with email/password+MFA OR Google OAuth+MFA". Resolving logical consequences.
- **Ambiguity resolution:** "Fast response times" → "Response times <200ms at P99". Quantifying qualitative requirements.
- **Dependency ordering:** Topological sort of spec requirements to eliminate circular dependencies in generation.
- **Constitutional constraint checking:** Verify that lower-level specs satisfy higher-level architectural constraints (GitHub Spec-Kit's constitutional approach).

**DSPy as spec optimizer:** DSPy's optimizers literally optimize programs (which are specs for LLM behavior) by searching prompt space. GEPA (genetic optimization) treats the spec as a search object and evolves it toward better metric performance. This is the first operational implementation of spec optimization.

**The limit of spec optimization:** Unlike code, specs contain implicit knowledge that the optimizer may not be able to infer. Optimizing "Users can search by name or email" to "Users can search by name, email, or phone" adds new information that wasn't in the original spec. Valid code optimization is purely local (no new semantics introduced); valid spec optimization may require external domain knowledge.

### 4.4 Target Language Selection: Determinism and What Changes

**Same spec → Python vs. Go:** Does the spec change? The claim is no — the spec is target-language-agnostic. But implementation research shows this is false in practice:

- Go requires explicit error handling at every callsite. A spec that says "handle errors appropriately" will generate different patterns in Go (explicit checks) vs Python (exceptions) vs Rust (Result types)
- Python's duck typing means a spec can be satisfied by multiple incompatible implementations; Go's structural typing constrains the solution space
- JavaScript's async model (callbacks/promises/async-await) is implicit in any spec involving I/O; the generated code varies significantly

**What Tessl's spec format actually captures:** Behavioral contracts (what the system does), not implementation patterns (how it does it). The `@generate` tag is the compilation directive — the agent must produce target-language-appropriate code while satisfying the behavioral contract. This is cross-compilation in the formal sense.

**Determinism across targets:** Even with a fixed spec, different target languages produce behaviorally non-equivalent code for edge cases. Example: integer overflow is silent in C, panics in Rust, throws in Python, wraps in Go. A spec that doesn't address overflow behavior will be compiled differently across targets. The spec is underspecified relative to the full behavioral space.

**PLDI 2025: "Scalable, Validated Code Translation of Entire Projects using LLMs"** (Zhang, David, Wang, Paulsen, Kroening): Addresses exactly this problem — translating whole codebases between languages while maintaining behavioral equivalence. Key result: requires formal validation at the unit test level to detect translation errors. Pure LLM translation without validation fails on ~15-20% of non-trivial cases.

---

## 5. Historical Deep Dive

### 5.1 Intentional Software: What Actually Happened

**Origin:** Charles Simonyi began Intentional Programming research at Microsoft Research in the late 1990s. The core idea: code should be stored as an *abstract semantic model* (AST-like structure), with multiple projections/views generated for different audiences. A pension formula could be displayed as mathematics to actuaries, as code to engineers, as English to compliance officers — all from the same semantic artifact.

**Why Microsoft killed the original project (circa 2001):** The timing coincided with .NET launch. Microsoft was rolling out C# and .NET to counter Java adoption and decided not to productize Intentional Programming — it was too radical a paradigm shift. Simonyi obtained IP rights and founded Intentional Software in 2002.

**Why Intentional Software failed to achieve mainstream adoption (2002-2017):**
1. **Secrecy:** The product was kept under NDA for years. No public release meant no community building, no open-source contribution, no ecosystem
2. **Demo-to-product gap:** Martin Fowler's 2009 review ("HOLY CRAPOLA") was based on compelling demos. "No system designed using the Intentional Domain Workbench has yet gone live" — years of impressive technology with zero production deployments
3. **Proprietary ecosystem:** Locked to CLR/C#. Cross-platform support came too late
4. **Complexity cliff:** Building a custom language workbench required specialized expertise far beyond typical engineering teams
5. **No LLMs:** The key technology that would have made Intentional Programming tractable — LLMs that can translate between representations — did not exist. Human-maintained multiple projections was too expensive

**Microsoft acquisition (April 2017):** Employees folded into Microsoft Office team. The IP was absorbed rather than productized. Post-acquisition, Simonyi continued working on "future productivity scenarios" but no public products emerged. The projectional editing technology was likely applied to internal Microsoft tooling.

**The lesson for today:** Intentional Software failed because it required humans to maintain multiple consistent projections of the same semantic model. LLMs can generate projections on-demand — code from spec, documentation from spec, tests from spec. This makes the Intentional Programming model suddenly tractable. Tessl's spec-as-source is the closest current implementation of Simonyi's original vision.

**What succeeded instead: JetBrains MPS.** Open-source, productized, deployed in multiple industries.

### 5.2 JetBrains MPS: The Surviving Projectional Editor

MPS (Meta Programming System) is the only projectional editor with significant real-world adoption. Current state (2025):

**Active development:** MPS 2025.1, 2025.3, and 2025.3 EAP2 all released within 2025. Active release cadence matches JetBrains' IntelliJ platform cycle. MPS 2025.3 introduced JavaDoc language overhaul, generator plans changes, reflective editor update, keyboard actions in Logical view.

**Industry adoption:** MPS-based DSLs deployed in:
- Electrical engineering (Siemens)
- Insurance industry (policy modeling)
- Tax legislation (legislative modeling, several EU countries)
- Healthcare (medical device specification)
- Bioinformatics (data analysis pipelines)
- Embedded software (automotive, aerospace)

**Community:** "Small compared to mainstream technologies but very active." No public user count, but the specificity and diversity of domains suggests genuine production use. JetBrains presents MPS as an enterprise tool, not a consumer product.

**Why MPS worked where Intentional Software failed:**
1. Open-source from early on → community, ecosystem
2. JetBrains distribution through existing IntelliJ channels
3. Java/JVM ecosystem (largest developer base at time of launch)
4. Concrete production deployments in niche domains before broad claims
5. Focused on DSL building (narrow scope) not general programming (broad claim)

**MPS's limitation:** Still requires expert DSL designers. The cognitive overhead of projectional editing is real — UX research shows ~3 days to overcome initial discomfort. Non-editors (domain experts who aren't developers) struggle. This is the same failure mode as all previous language workbench attempts: the tools for domain experts require expert developers to build and maintain.

**MPS and LLMs (2025):** No official MPS-LLM integration. An academic paper (GRAPE, ISWC 2025) uses MPS for RML authoring with a projectional editor. The natural evolution — using LLMs to generate MPS languages from domain descriptions — hasn't been officially implemented but is an obvious direction.

### 5.3 Wolfram Language: The NL Programming Limit Case

Wolfram Language represents the most sophisticated pre-LLM attempt at NL-inspired programming with formal computation. Key lessons:

**What worked:** Wolfram|Alpha's NL interface for computational queries. The model works by translating NL → Wolfram Language → computation → result. This is exactly the NL→PL→execution pipeline that today's systems aspire to. It worked in the narrow domain of mathematical and scientific computation where:
- Queries have unique correct answers
- Domain has formal structure that maps cleanly to NL
- User intent is precise (mathematical questions have deterministic correct answers)

**What failed:** Scaling NL programming beyond mathematical computation. Wolfram Alpha "expects fairly well-formed input and often fails on ambiguous or conversational phrasing." For complex multi-step programs, NL becomes unworkable — "just like in mathematics without notation, it quickly becomes impractical." Wolfram's own assessment (Writings, 2010): NL programming would eventually work but requires formal intermediate representations.

**The LLM integration (2023-2025):** Wolfram integrated with ChatGPT (plugin) and provides LLM access to Wolfram Language. Modern LLMs extend Wolfram's NL understanding by enabling conversational queries that get translated to Wolfram Language calls. The architecture: LLM parses intent, Wolfram Language executes it. This is the "LLM as semantic parser" model — the LLM doesn't compute, it translates.

**Key lesson:** Even with 40 years of Wolfram Language development and a massive knowledge base, NL programming is limited to specific domains. The LLM era doesn't eliminate this limit — it extends it to larger domains through better semantic parsing, but the fundamental constraint (NL is ambiguous for complex programs) remains.

### 5.4 Low-Code/No-Code History: What Worked and What Didn't

**The platform generation:**
- OutSystems (2001): Worth $9.5B. Enterprise focus, professional developers
- Mendix (2005): Acquired by Siemens for $700M (2018)
- Appian: Peak $234/share (Feb 2021), dropped to ~$33 by late 2022 (-86%)

**The core failure:** The "citizen developer" claim. LCAPs (Low-Code Application Platforms) claimed non-developers could build enterprise applications. Mendix later admitted "anything more complex than a basic CRUD system still requires a professional software engineer." This is the same claim made about "vibe coding" in 2024-2025.

**Specific technical failures identified:**
1. Visual microflow development slower than code in an IDE for complex logic
2. Microflows become unmanageable past a certain complexity threshold (vendor lock-in prevents refactoring tools)
3. Proprietary formats preventing migration — full system rewrites required to exit the platform
4. Performance limitations: generated code couldn't match hand-optimized code for high-throughput scenarios

**What actually worked:**
- CRUD applications and workflow automation — the original use case
- Enterprise connectivity (integrating legacy systems through visual connectors)
- Business analyst empowerment for *simple* applications
- Regulatory compliance workflows where auditability > performance

**The SDD parallel:** Every SDD practitioner acknowledges the same limits: "SDD may be overkill for throwaway prototypes, solo short-lived projects, and simple CRUD applications with obvious requirements." The failure mode is identical — tools designed for complex systems get oversold as universal solutions, then struggle at scale.

**The market reality:** Low-code didn't replace developers. It added a new layer of tooling that developers maintain. The total development labor didn't decrease — it redistributed. SDD will likely follow the same pattern.

### 5.5 DSL Failure Modes: Why They Didn't Replace GPLs

**The adoption paradox:** DSLs are technically superior for their domain (SQL for relational queries, Terraform for infrastructure, regex for pattern matching). Yet no DSL has replaced a GPL. Root causes:

**Developer resistance:** Research explicitly identifies "resistance from seasoned developers who fear the DSL lowers the bar by being simpler to use, and a new DSL is threatening because it reduces the importance of some of their skills." This is not irrational — DSLs genuinely do reduce the value of general programming skills in their domain.

**The AT&T 5ESS lesson (DSL research classic):** A DSL evolved from an earlier imperative DSL, which replaced C + English. Key finding: "domain-specific languages should not be designed to describe *computation*, but to express useful facts from which computation can be derived." This is the spec-driven development principle stated in 1990s DSL research — express intent, not mechanism.

**GPL vs DSL as false binary:** The most successful "DSLs" are embedded in GPLs — SQL embedded in Python, HCL embedded in JSON, YAML for configuration. Pure external DSLs fail because they require separate toolchains, training, and mental model switches. Embedded DSLs succeed because they compose with existing ecosystems.

**The AI era implication:** NL specs are the ultimate embedded DSL — embedded in natural language itself, requiring no new syntax learning. But they inherit the fundamental DSL limitation: they work where domain concepts map cleanly to NL semantics, and fail where complex program logic requires non-NL constructs (recursion, concurrency, complex state machines).

**GPL's durability:** GPLs survived DSL pressure for 40 years because:
1. Network effects (tooling, libraries, community)
2. Composability (any DSL task eventually needs to interface with general-purpose code)
3. Talent supply (you hire Python developers, not Terraform specialists)
4. Escape hatches (when DSL fails, you drop to GPL)

Escape hatches are critical — and currently missing from NL spec systems. What happens when CodeSpeak or Tessl cannot express a requirement? You need to drop to code. This reintroduces the dualism that spec-driven development was meant to eliminate.

---

## 6. The Trust/Verification Gap

### 6.1 Verification Overhead: Generated vs Hand-Written Code

**The SAGA framework (arXiv 2507.06920, ICLR 2026 submission):** Directly measures verification gap. LLM-generated solutions that passed LiveCodeBench's private test suite showed:
- 20% failure rate on medium problems when re-evaluated on LeetCode's online judge
- 40% failure rate on hard problems

This is the verification overhead: generated code that appears correct (passed test suite) fails in production at 20-40% rates on non-trivial problems.

**Why existing test suites fail to detect LLM bugs:** Test suites are generated by LLMs or based on problems written for human coders. LLMs make systematically different errors than humans (different blind spots, different failure modes). Tests built on human error patterns miss LLM-specific bugs.

**SAGA's approach:** Combine human expertise (correct solutions as reference) with LLM reasoning (differential analysis of incorrect submissions). Detection rate: 90.62% of errors detected vs <82% for baseline methods.

**Key theoretical finding:** Adding more tests yields diminishing returns due to inter-test correlation. There exists a detection rate ceiling below 100% for any fixed test suite — you cannot test your way to certainty for complex programs.

**Meta's mutation testing deployment (January 2026):** Meta applies LLMs to mutation testing at scale via their Automated Compliance Hardening (ACH) system. LLM-generated mutants + tests replace traditional mutation testing (previously limited by cost and equivalent mutant explosion). LLM-based equivalence detector filters redundant mutants. This is the current state-of-the-art for generated code verification at production scale.

**Quantified trust gap:** Only 3% of developers highly trust AI-generated code. 71% refuse to merge without manual review. Average mutation scores: 40.21% for real-world benchmarks (LLM-generated tests) vs 50.80% for simpler benchmarks. Security/design flaw rate: 40-62% in newer models. This is not a rounding error — it is a fundamental trust deficit requiring structural solutions.

### 6.2 Trust Metrics for Generated Code

**Current metrics in use:**
- Pass rate on test suites (inadequate — see §6.1)
- Mutation score (better, but LLM-generated tests have ~40% mutation scores vs ~50% for human-generated)
- Static analysis findings per KLOC (comparable to human code, but different finding types)
- Cyclomatic complexity (AI code is 40%+ more complex than equivalent human code)

**SAGA's proposed metrics:**
- Detection Rate (DR): probability that a test suite catches errors in known-incorrect solutions
- Verifier Accuracy: fraction of all incorrect solutions correctly identified
- Distinct Error Pattern Coverage (DEPC): diversity of error patterns covered
- AUC-AccN: accuracy across N difficulty levels

**The trust calibration problem:** Developers report 24% productivity increase expectations, see 19% slowdown (METR study), but 71% still believe they got faster. This miscalibration (perceived 20% faster, actual 19% slower) is dangerous for verification decisions — developers systematically overestimate the trustworthiness of AI-generated code.

### 6.3 Legal Liability: Who Is Responsible?

**Current legal landscape:** Courts have not established clear precedent for AI-generated code failure liability. Primary legal frameworks:

**Product liability:** The developer who ships software is liable, regardless of how it was generated. AI tool providers' disclaimers ("AI can make mistakes — verify the output") transfer burden to the integrating organization. This is the current de facto standard.

**Intellectual property:** AI-generated code lacking "meaningful human authorship" is not copyrightable. Organizations that ship AI-generated code may not own the copyright — a significant commercial risk. The threshold for "meaningful human authorship" is undefined in case law.

**Practical liability allocation:**
1. AI tool provider: may face liability for systematic flaws if tool is marketed as reliable
2. Developer: primary responsibility for code shipped
3. Organization: secondary responsibility for policies and oversight

**Insurance gap:** No specific AI-generated code insurance products exist. Standard E&O (Errors & Omissions) policies may not cover "AI-related failure" — language is ambiguous.

**The spec-compiled code question:** If a formal spec is provided and code is generated from it, does liability shift toward the spec author? No legal precedent. The closest analogy: CAD-generated manufacturing files. If a CAD design is correct but the CNC machine generates defective parts, liability depends on the design-to-manufacture verification step. The same logic suggests: correct spec + incorrect generation → liability lies in the generation verification failure.

### 6.4 Safety-Critical Systems: The DO-178C Gap

**NASA's assessment (2025 report):** "Examining Proposed Uses of LLMs to Produce or Assess Assurance Arguments" identifies fundamental gaps:

1. **Plausibility vs. accuracy:** LLMs aim for plausible-sounding answers, not verified facts. A single invented citation could invalidate an entire certification package
2. **Fabricated evidence:** LLMs have been documented inventing references, misquoting regulations, overlooking corner-case hazards in safety analyses
3. **Non-reproducibility:** Certification requires repeatable evidence; LLM outputs are not reproducible

**DO-178C specific requirements that LLMs cannot satisfy independently:**
- Every claim must trace to objective, verifiable evidence (test results, static analysis, coverage metrics)
- Assurance arguments structured in Goal Structuring Notation (GSN) require human validation
- Coverage metrics (MC/DC for DAL-A/B) require deterministic test execution records
- The "qualified human engineer" must sign off on all assurance arguments

**Parasoft's recommended approach for safety-critical LLM use:**
1. Deterministic tools (static analysis) run first — results are evidence
2. LLM rephrases/explains vetted findings only — never generates novel claims
3. Human engineer reviews all LLM outputs line-by-line
4. Evidence anchoring: every AI suggestion must link to ground-truth artifacts

**What certification would actually require for LLM-generated code:**
- Formal proof of equivalence between spec and generated code (currently: type checking + tests, insufficient for DO-178C)
- Deterministic generation (achievable at 62% performance penalty per §2.3)
- Fixed model version with no silent updates (incompatible with commercial APIs)
- Tool qualification under DO-178C Section 12 (no LLM has been qualified)

**Realistic timeline for safety-critical LLM use:** Industry estimate: 10+ years for avionics (DO-178C qualification process takes 5-7 years for new tools). Medical devices (FDA): 5-7 years (FDA exploring LLM tagging framework, not yet approving LLM-generated code for SaMD). Automotive (ISO 26262): 3-5 years for ASIL-B/C applications with human oversight.

---

## 7. Developer Experience Research

### 7.1 How Developers Actually Write Specs

**Empirical evidence from SDD practitioners (Augment Code, Kiro user reports):**

The primary challenge is not technical — it is cognitive. Developers struggle with:
1. **Level calibration:** "Frequent confusion about when to stay on the functional level versus adding technical details." Too abstract → agent makes wrong design decisions. Too concrete → spec becomes code with extra steps
2. **Completeness judgment:** Knowing when the spec is complete enough to generate. No formal completeness criterion exists
3. **Edge case discovery:** Specs naturally omit edge cases. Agents implement the happy path and miss error handling that developers would automatically include
4. **Spec drift awareness:** Developers lose track of which generated code matches which spec version after multiple iterations

**Pattern from Tessl's 2025 Year in Review:** The pivot from vibe coding to spec-driven development required changing developer mental models more than tools. "The uncomfortable truth about vibe coding" (Red Hat Developer, Feb 2026): vibe coding is addictive because it produces fast visible results, even when those results are wrong. Spec writing is slower upfront and delayed gratification.

**One practitioner case study (186 tasks, 94% test coverage):** Achieved via:
1. Constitutional specs established first (architectural invariants)
2. Hierarchical decomposition (system → module → function level specs)
3. Test-first spec validation (write the test expectation in the spec before generating implementation)
4. Atomic spec updates (one behavior per spec update, re-generate, verify, commit)

The developer described this as "more like writing tests than writing code" — the mental model shift is from imperative ("do this") to declarative ("this must be true").

### 7.2 Learning Curve: How Long to Become Productive?

**Kiro's Spec Kit overhead (observed):** 1-2 hours to generate and refine spec artifacts before implementation begins. This is amortized across the implementation — but for small tasks (<4 hours), the overhead is not justified.

**JetBrains MPS UX research (analogous tool):** "It takes a few days for most users to become accustomed to the projectional editor and feel no discomfort, with many users claiming they prefer the MPS way of editing things." 3-5 days to basic proficiency, weeks to months for expert use.

**DSPy learning curve (Stanford NLP reports):** Signatures and modules are Python-native — developers familiar with Python are productive within hours for simple pipelines. Complex optimization workflows (MIPRO, GEPA) require 1-2 days to understand effectively.

**Estimate for SDD tools:** 1-2 weeks to basic proficiency (writing specs that generate working first-pass code consistently). 1-2 months to production-grade proficiency (writing specs that generate code meeting quality bar without major revision cycles). Expert level (writing specs for complex multi-service systems): 6+ months.

**The 19% slowdown context:** The METR study measured experienced developers on a task set over a short period. The SDD learning curve is front-loaded — initial slowdown followed by productivity gains as spec patterns become internalized. Whether the long-term productivity curve is positive or negative is unknown — no longitudinal study exists.

### 7.3 Cognitive Load: Is Writing Specs Harder or Easier Than Code?

**The spec-writing cognitive task differs structurally from code-writing:**

Code-writing cognitive load:
- **Germane:** Understanding the domain problem (constructive)
- **Intrinsic:** Language syntax, type system, standard library (inherent to task)
- **Extraneous:** Variable naming, formatting, boilerplate (wasted)

Spec-writing cognitive load:
- **Germane:** Understanding the domain problem (same)
- **Intrinsic:** Spec format, level calibration, completeness judgment (reduced vs code)
- **Extraneous:** Iteration loops to correct misinterpretations (new cost)

**The tradeoff:** Spec writing eliminates intrinsic programming load (syntax) but introduces new intrinsic load (level calibration, completeness judgment). The extraneous load changes character — from boilerplate to iteration cycles. Net cognitive load is likely lower for *expert spec writers* who have internalized calibration, higher for beginners who haven't.

**Empirical reference:** The ThoughtWorks analysis (McKinsey data, 2025): "organizations are achieving 20-45% gains in developer productivity through AI tools that reduce cognitive load, speed prototyping, and minimize downstream rework." Company-wide gains average only 5-15% — the productivity gains don't fully propagate. This delta (20-45% individual, 5-15% company) suggests coordination and verification overhead absorbs the individual gains.

### 7.4 IDE Support: What Features Are Needed for NL Specs?

**Currently available (2025-2026):**
- Syntax highlighting for `.spec.md` files (basic, Tessl extension)
- Agent invocation from spec files (Kiro, Tessl, GitHub Spec Kit)
- Inline diff between spec and generated code (nascent, Tessl exploring)
- Spec registry package manager (Tessl, CLI-only)

**Critical missing features:**
1. **Spec-level type checking:** Detecting spec contradictions and incompleteness before generation. Currently: no tool does this formally. Informal: some tools use LLMs to review specs before code generation
2. **Semantic refactoring:** Renaming a concept in the spec (e.g., "customer" → "client") and having all generated code update automatically. Currently: requires manual regeneration
3. **Spec coverage:** Which parts of the spec are covered by tests? Which generated code has no spec coverage? This is the spec-analogue of code coverage — not yet implemented anywhere
4. **Spec diff:** When a spec changes, show what behaviors changed. Currently: no tool provides this
5. **Inline spec validation:** As you type a spec, real-time feedback on ambiguity and incompleteness. Currently: possible only through LLM-in-the-loop validation (slow, expensive)
6. **Cross-spec dependency graph:** Visualize how spec modules depend on each other. Currently: Tessl tracks dependencies but no visualization

**PLDI 2025: "Programming by Navigation"** (Lubin, Ziegler, Chasins): A synthesis approach enabling "expressive, exact, and efficient program generation through navigational interfaces." The insight: instead of specifying programs as text descriptions, users navigate a structured space of program possibilities. This is the spec IDE of the future — interactive, bidirectional, structured.

**Hazel's contribution:** Live functional programming with typed holes directly addresses the IDE gap for NL specs. If the spec editor can typecheck incomplete specs (analogous to typed holes in Hazel), developers get immediate feedback before full code generation — reducing the iteration cycle from minutes to seconds.

---

## 8. Economic Model

### 8.1 Cost Comparison: Spec Writing vs. Code Writing

**Direct cost (developer time):**

Traditional development hourly breakdown (estimate from McKinsey research):
- Requirements analysis: 15%
- Design: 10%
- Implementation: 30%
- Testing: 25%
- Documentation: 10%
- Review/maintenance: 10%

SDD model (arXiv 2602.00180 + Tessl data):
- Spec writing (requirements + design combined): 30-40%
- Spec review and validation: 10-15%
- Code generation and verification: 10-15%
- Testing (spec-generated tests + manual review): 20-25%
- Maintenance: estimated lower (spec is the maintained artifact)

**Surprising cost shift:** Spec writing + review takes *more* time upfront (30-40% vs 25% for requirements+design). The saving is in implementation (10-15% vs 30%). Net saving in development phase: ~15-20%. Testing may also decrease if specs generate more reliable code — but current evidence (40-62% defect rate) suggests testing costs may not decrease significantly.

**Per-function cost data (arXiv 2602.00180):** "Human-refined specs significantly improve LLM-generated code quality, with controlled studies showing error reductions of up to 50%." A 50% error reduction in generated code could halve testing/debugging costs — but testing costs are not the largest cost in most development projects.

**The financial services case study:** 75% reduction in integration cycle time using OpenAPI specifications (arXiv 2602.00180). This is the strongest empirical evidence for SDD economic value — but it applies specifically to well-formalized domains (API integration) where specs have formal semantics (OpenAPI is machine-readable).

### 8.2 Maintenance Cost: Spec vs. Code Over Time

**The maintenance thesis:** Code grows in complexity over time (entropy). Specs remain stable or grow slowly. If code can be regenerated from specs, you maintain the spec (small, stable) not the code (large, complex).

**Counter-evidence:**
1. Spec drift is a real problem — specs and code diverge without active enforcement
2. Context window limits mean large spec sets require chunking that can introduce inconsistencies
3. Tessl's approach (spec-as-source with `@generate`) addresses drift but requires non-trivial tooling
4. When specs drift from code, debugging requires reading both — double the surface area

**Token context economics:** When context usage exceeds 40%, AI performance degrades significantly. A large project with many interconnected specs will hit this limit. The spec must be partitioned — but partitioning introduces seams where inter-spec inconsistencies can hide.

**Long-term maintenance prediction (based on historical analogies):**
- Short-term (0-2 years): Maintenance costs likely higher than traditional (learning curve, tooling immaturity)
- Medium-term (2-5 years): Comparable to traditional if tools mature and spec hygiene is maintained
- Long-term (5+ years): Potentially lower — specs are smaller, more expressive, easier to reason about than equivalent code

The key uncertainty: whether spec hygiene can be maintained by organizations that struggle to maintain code documentation today. If organizations couldn't maintain Confluence pages (documentation), why would they maintain specs?

### 8.3 Team Size Impact: Where SDD Works Best

**Current practitioner consensus (Tessl, Kiro, Augment Code data):**

SDD works best:
- Complex multi-service systems (many components with defined interfaces)
- Teams where domain experts and developers are separate (domain experts write specs, developers verify)
- Projects where requirements are stable enough to spec upfront
- New projects (greenfield) where no existing codebase creates friction

SDD struggles:
- Small/solo projects (overhead > benefit)
- Rapidly evolving requirements (specs become stale before code is generated)
- Brownfield development (existing code doesn't match spec; OpenSpec's delta approach partially addresses this)
- Teams without spec-writing discipline (same teams that don't write good comments)

**Team size data:** The "one-pizza pod" model (McKinsey 2025): 3-5 person teams with AI agents replace traditional 8-10 person teams. But this reduction is in code-writing labor, not in specification labor. The hidden cost: verification and spec writing are both knowledge-intensive and harder to automate away.

**The "flight levels" model:** Andrew Clay Shafer's formulation: organizations need people at three levels:
1. Strategy level: what to build (domain experts, product managers)
2. Coordination level: how teams align (architects, leads)
3. Operations level: how to build it (developers, agents)

SDD potentially eliminates operations-level developer labor (code writing) while increasing coordination-level work (spec writing, agent orchestration). Net headcount impact: possibly zero, with skill mix shift.

### 8.4 Hiring Impact: Domain Experts vs. Coders

**The emerging hiring shift:**

Traditional team: 1 product manager, 2-3 backend developers, 1-2 frontend developers, 1 QA engineer

SDD-optimized team prediction (based on "one-pizza pod" data): 1-2 spec authors (domain experts with technical literacy), 1 agent orchestrator (validates generated code, manages AI tooling), 1 QA/verification specialist (tests, security)

**What "technical literacy" means for spec authors:** The ability to write unambiguous behavioral specifications — more like technical writing than programming, but requiring enough technical knowledge to specify edge cases, security requirements, performance constraints. This is closer to a technical business analyst or a software architect than to a developer.

**The retraining question:** Are current developers well-positioned to become spec authors? Evidence: developers with strong system design skills (architects, seniors) transition well. Junior developers who primarily write boilerplate code struggle more — their existing skill (implementation) is the one being automated.

**Hiring market shift signals (early 2026):**
- "Technical writer with AI tool experience" job postings up significantly
- "Senior software architect" demand increasing
- "Junior developer" demand declining in AI-forward organizations
- "AI agent operator" emerging as a new job category

This is the workforce transition the industry didn't predict from vibe coding's emergence: not "anyone can code" but rather "coding skill is redistributing up the abstraction ladder."

---

## 9. The Compilation Model: Formal Synthesis

### 9.1 NL→LTL as Foundation for Verified NL Compilation

The strongest current result for NL spec compilation:

**Req2LTL (arXiv 2512.17334):** 88.4% semantic accuracy on real-world aerospace requirements. The two-stage architecture:
1. LLM decomposes NL → OnionL (hierarchical IR: scopes, relations, atomic propositions)
2. Rule-based engine synthesizes OnionL → LTL

**Why the hybrid approach works:** LLMs excel at local semantic extraction (what does "until" mean in context?). Rule-based systems excel at global logical composition (how do temporal operators compose?). Error propagation from LLM mistakes is contained by the formal synthesis step.

**ConformalNL2LTL (arXiv 2504.21022):** The first system with statistical correctness guarantees. Uses conformal prediction to achieve user-specified success rates. Example: "give me 90% confidence that this NL requirement is correctly translated." This is calibrated uncertainty quantification for NL compilation — allowing users to choose their risk tolerance.

**LTLGuard:** Modular toolchain combining constrained generation + formal consistency checking. Generates conflict-free LTL specifications from informal input. Key feature: detects contradictions between specifications before code generation — equivalent to type checking at the spec level.

### 9.2 PLDI 2025 Highlights for NL Programming

**Type-Constrained Code Generation with Language Models** (Mündler et al.): Type-guided decoding reduces compilation errors by >50% and significantly increases functional correctness. This is the strongest argument for rich type systems in the AI era: type information during generation is not just documentation — it actively constrains the generation space toward correct code.

**Program Synthesis From Partial Traces** (Ferreira et al.): Generate programs from incomplete execution traces, reducing specification burden. If users can demonstrate behavior rather than describe it, specification becomes more accessible — behavioral examples as specs, not NL text.

**Programming by Navigation** (Lubin et al.): Synthesis via navigating a structured program space rather than text description. This is a completely different UX model for specification — structured choice rather than free-form text.

**Neurosymbolic Program Synthesis** (Chaudhuri): NSP framework combining neural guidance (LLMs) with symbolic reasoning (formal constraints). 2025 overview covers applications in image editing, data extraction, robot learning. The neurosymbolic approach is the theoretical framework that subsumes the LLM-as-library metaphor: LLMs provide neural guidance over a search space constrained by formal specifications.

### 9.3 LMPL 2025: Emerging Research Directions

The first Language Models and Programming Languages workshop (LMPL 2025, Singapore, October 2025, co-located with ICFP/SPLASH) surfaced research directions that will define the next 3-5 years:

**Hallucination-Resilient Static Analysis:** LLMs with sound, tunable static analysis properties. Addresses the fundamental trust problem — static analysis provides guarantees, LLMs provide scale, the combination provides both.

**Repository-Level Verification:** Extending LLM verification from functions to entire repositories. This is the spec-as-source vision extended to verification — not just generating code from specs, but verifying entire codebases against specs.

**Algebraic Effect Handlers for LLM Programs** (Tan et al., Berkeley/CMU): Programming LLMs using algebraic effects and the selection monad — bringing functional programming's principled composability to LLM programs. This is the most theoretically ambitious proposal: treating LLM programs as functional programs with explicit effects, enabling compositional reasoning.

**Modular Imperative for LLMs** (LMPL 2025): Position paper arguing for software engineering principles (modularity, abstraction, interfaces) to govern LLM-powered systems. The core claim: current LLM integration is ad hoc and will not scale. Principled module systems are necessary.

---

## 10. Synthesis: What the Second Pass Adds

### 10.1 The Compilation Stack Is Emerging

There is now a coherent (if not yet integrated) stack for NL compilation:

```
Domain Expert Intent (free NL)
    ↓ Req2LTL [88.4% accuracy, 100% syntactic correctness]
Formal Behavioral Contracts (LTL, On ionL IR)
    ↓ LTLGuard [conflict-free synthesis]
Verified Specifications (.spec.md, EARS notation)
    ↓ Multi-pass SDD [Kiro/Tessl/Spec-Kit]
Implementation Tasks (atomic, verifiable units)
    ↓ Type-Constrained Generation [PLDI 2025, >50% error reduction]
Source Code (with typed interfaces, linear types, compile-time reflection)
    ↓ SAGA/CodeCompass [90.62% error detection]
Verified Implementation
```

No single tool implements this entire stack. Different pieces are at different maturity levels. The production gap is 3-5 years for business applications, 10+ years for safety-critical.

### 10.2 Historical Patterns Repeat with Different Outcomes

**Intentional Software failure (2002-2017) → Tessl (2024-):**
Same core idea (spec-as-source, multiple projections), different enabling technology (LLMs replace manual projection maintenance). The same vision that failed before is now technically tractable.

**Low-code/no-code oversell (2010s) → vibe coding (2024-2025):**
Same failure mode — claiming democratization, delivering narrow domain solutions. SDD is the discipline that emerges after the oversell cycle.

**DSL adoption failure → NL specs:**
DSLs failed because they required new syntax and new mental models. NL specs succeed in the same niches (well-defined behavioral domains) but are more accessible because NL requires no syntax learning. The escape hatch problem (when DSL fails, drop to GPL) is the remaining unsolved issue for NL specs.

**Wolfram Language limit → LLM+Wolfram:**
NL programming for mathematical computation was solved. LLMs extend the domain but don't change the fundamental limit: complex programs cannot be expressed in NL alone. The hybrid model (NL for intent, formal language for computation) is now implemented by LLM-as-semantic-parser + Wolfram Language.

### 10.3 The Three Unsolved Problems

1. **Determinism:** Making same spec → same code consistently. Technically solvable (batch-invariant inference at 62% overhead) but economically unviable for cloud APIs. Requires private deployment and model pinning.

2. **Compositional verification:** Proving that composed NL modules satisfy end-to-end contracts. Research-stage (LMPL 2025, algebraic effects). No production implementation.

3. **Escape hatch:** What happens when the spec cannot be implemented by the LLM? Current tools require dropping to code, reintroducing the code maintenance problem. Principled escape hatch mechanisms don't exist.

Until these three are solved, NL compilation is a productivity tool with limited formal guarantees, not a replacement for programming languages.

---

## Sources

### New Languages and Frameworks
- Mojo 1.0 roadmap: https://docs.modular.com/mojo/roadmap/
- Modular 26.1 release: https://www.modular.com/blog/modular-26-1-a-big-step-towards-more-programmable-and-portable-ai-infrastructure
- Modular: Do LLMs eliminate need for PLs: https://www.modular.com/blog/do-llms-eliminate-the-need-for-programming-languages
- MojoBench (arXiv 2410.17736, NAACL 2025): https://arxiv.org/abs/2410.17736
- Mojo-Coder HuggingFace: https://huggingface.co/md-nishat-008/Mojo-Coder
- DSPy Stanford NLP: https://dspy.ai/
- DSPy ICLR 2024 paper: https://openreview.net/pdf?id=sY5N0zY5Od
- SimPy/DualCode (arXiv 2404.16333, ISSTA 2024): https://arxiv.org/abs/2404.16333
- SynCode (arXiv 2403.01632): https://arxiv.org/abs/2403.01632
- AIOS Compiler / CoRE (arXiv 2405.06907): https://arxiv.org/abs/2405.06907
- Roc programming language: https://www.roc-lang.org/
- Hazel live programming environment: https://hazel.org/
- Hazel OOPSLA 2025 (Incremental Bidirectional Typing): https://arxiv.org/abs/2506.10781
- Gleam programming language: https://gleam.run/
- MoonBit AI-friendly PL (ACM LLM4Code 2024): https://dl.acm.org/doi/10.1145/3643795.3648376
- MoonBit 1.0 roadmap: https://www.moonbitlang.com/blog/roadmap

### LLM-as-Library: Determinism and Caching
- vLLM reproducibility docs: https://docs.vllm.ai/en/latest/usage/reproducibility/
- vLLM batch invariance: https://docs.vllm.ai/en/latest/features/batch_invariance/
- Thinking Machines Lab determinism: https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference/
- Keywords AI consistency guide 2025: https://www.keywordsai.co/blog/llm_consistency_2025
- Semantic caching Redis guide: https://redis.io/blog/what-is-semantic-caching/
- GPTCache (Zilliz): https://github.com/zilliztech/GPTCache
- RAG vs code generation (arXiv 2503.20589): https://arxiv.org/abs/2503.20589
- RACG survey (arXiv 2510.04905): https://arxiv.org/html/2510.04905v1

### NL Specification and Formal Methods
- Req2LTL / NL-to-LTL (arXiv 2512.17334): https://arxiv.org/abs/2512.17334
- Grammar-Forced LTL translation (arXiv 2512.16814): https://arxiv.org/abs/2512.16814
- ConformalNL2LTL (arXiv 2504.21022): https://arxiv.org/abs/2504.21022
- LTLGuard (arXiv 2603.05728): https://arxiv.org/html/2603.05728
- AIOS LTL safety framework (arXiv 2503.15840): https://arxiv.org/abs/2503.15840

### SDD Ecosystem: Tools and Economics
- arXiv SDD formal paper (arXiv 2602.00180): https://arxiv.org/html/2602.00180v1
- Tessl how products pioneer SDD: https://tessl.io/blog/how-tessls-products-pioneer-spec-driven-development/
- Tessl spec registry launch: https://tessl.io/blog/tessl-launches-spec-driven-framework-and-registry/
- Tessl 2025 year in review: https://tessl.io/blog/a-year-in-review-from-vibe-coding-to-viable-code/
- Kiro and future of software: https://kiro.dev/blog/kiro-and-the-future-of-software-development/
- Augment Code SDD tool comparison: https://www.augmentcode.com/tools/best-spec-driven-development-tools
- ThoughtWorks on SDD: https://www.thoughtworks.com/insights/blog/agile-engineering-practices/spec-driven-development-unpacking-2025-new-engineering-practices
- Red Hat SDD quality: https://developers.redhat.com/articles/2025/10/22/how-spec-driven-development-improves-ai-coding-quality
- The New Stack: vibe coding vs spec-driven: https://thenewstack.io/vibe-coding-spec-driven/
- Marmelab: SDD is waterfall: https://marmelab.com/blog/2025/11/12/spec-driven-development-waterfall-strikes-back.html

### Trust, Verification, Legal
- SAGA verification framework (arXiv 2507.06920): https://arxiv.org/abs/2507.06920
- Meta LLM mutation testing (InfoQ, Jan 2026): https://www.infoq.com/news/2026/01/meta-llm-mutation-testing/
- Qodo state of AI code quality: https://www.qodo.ai/reports/state-of-ai-code-quality/
- CodeRabbit AI vs human code: https://www.coderabbit.ai/blog/state-of-ai-vs-human-code-generation-report
- MBHB legal landscape AI code: https://www.mbhb.com/intelligence/snippets/navigating-the-legal-landscape-of-ai-generated-code-ownership-and-liability-challenges/
- Parasoft/NASA safety-critical LLMs: https://www.parasoft.com/blog/addressing-nasa-concerns-llm-safety-critical-development/
- FDA AI-enabled medical devices: https://www.fda.gov/medical-devices/software-medical-device-samd/artificial-intelligence-enabled-medical-devices

### Historical
- Martin Fowler on Intentional Software: https://martinfowler.com/bliki/IntentionalSoftware.html
- Microsoft acquires Intentional Software: https://blogs.microsoft.com/blog/2017/04/18/microsoft-acquire-intentional-software-expand-future-productivity-capabilities/
- Intentional Software Wikipedia: https://en.wikipedia.org/wiki/Intentional_Software
- JetBrains MPS 2025.3: https://blog.jetbrains.com/mps/2025/12/mps-2025-3-is-out/
- MPS 2025.1: https://blog.jetbrains.com/mps/2025/05/mps-2025-1-is-out/
- Wolfram NL programming (2010): https://writings.stephenwolfram.com/2010/11/programming-with-natural-language-is-actually-going-to-work/
- Low-code history: https://fastgen.com/blog/the-history-of-low-code
- Low-code dangerous bet (Jmix): https://www.jmix.io/cuba-blog/low-code-platforms-a-dangerous-bet/
- DSL failure/adoption analysis: https://tomassetti.me/domain-specific-languages/
- GPL vs DSL AI evolution (DZone): https://dzone.com/articles/gpl-vs-dsl-ai-evolution

### Academic Conferences
- PLDI 2025 papers: https://pldi25.sigplan.org/track/pldi-2025-papers
- LMPL 2025 workshop: https://conf.researchr.org/home/icfp-splash-2025/lmpl-2025
- LMPL 2025 proceedings: https://dl.acm.org/doi/proceedings/10.1145/3759425
- POPL 2025: https://popl25.sigplan.org/track/POPL-2025-popl-research-papers
- Neurosymbolic Program Synthesis PLDI 2025: https://pldi25.sigplan.org/details/pldi-2025-papers/97/Neurosymbolic-Program-Synthesis-Bridging-Perception-and-Reasoning-in-Real-World-Appl
