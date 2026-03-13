---
title: "Code→Spec Reverse Engineering and Equivalence Verification"
date: 2026-03-12
tags: [code-summarization, formal-verification, program-equivalence, LLM, specification]
context: CodeSpeak / Andrey Breslav "Code Takeover" — reverse-engineering existing codebases into minimal NL specs
status: research
---

# Code→Spec Reverse Engineering and Equivalence Verification

Research prompted by CodeSpeak (Andrey Breslav, Kotlin creator) and their "Code Takeover" feature: take existing code, convert to minimal NL specs (~10x shorter), verify behavioral equivalence, support incremental spec edits. Five research questions addressed below.

---

## 1. Code Summarization — State of Art

### What the field calls "code summarization"

Code summarization (also: code comment generation, code documentation) produces NL descriptions of source code at varying granularity: line, function, class, file, repository. The task has shifted from small fine-tuned models (CodeT5, UniXcoder) to LLM-driven pipelines.

**Key 2025 survey (ICSE, Sun et al.)** — systematic study of LLM-based code summarization:
- Five prompting techniques tested: zero-shot, few-shot, chain-of-thought, critique, expert
- **Counter-intuitive finding**: advanced techniques do not reliably beat zero-shot. GPT-3.5 scores highest with zero-shot; GPT-4 scores highest with CoT; StarChat-β prefers CoT; CodeLlama prefers few-shot. No universal winner.
- LLMs consistently fail on **logic programming languages** (Prolog, Datalog) — the mode of reasoning doesn't match.
- LLM outputs score 3.9–4.2 in human evaluation vs. 3.0–3.5 for reference summaries → LLMs are often *better* than dataset ground truth.
- **Only GPT-4 as judge** aligns with human evaluation (correlation 0.28–0.65). BLEU/ROUGE-L are unreliable for assessing code summary quality.

### Abstraction levels and what works

**"Code Summarization Beyond Function Level" (Makharev et al., Feb 2025)** evaluated three levels:
- Function-level: baseline; understood well by LLMs
- Class-level: **class skeleton** (method signatures only) outperforms full class code — reduces noise
- Repository-level: requires RAG; DeepSeek Coder 6.7B improved BLEURT/BLEU4 at this level but at high compute cost

**Hierarchical bottom-up approach** (multiple 2025 papers): parse → function summaries → file summaries → package summaries. This is more reliable than monolithic summarization.

**Key finding on RAG**: adding repository chunks *without few-shot examples* provides "limited" benefit. Few-shot examples are the critical ingredient, not raw context.

**P2N2S (Springer 2025)**: two-stage approach — (1) convert code to line-level NL annotations via LLM, (2) run NLP summarizer on the annotations. By moving to a fully NL-grounded pipeline, the system avoids the mismatch between programming language parsing and NL generation.

### What abstraction level is useful for spec generation?

For CodeSpeak's purpose (spec ≠ docstring, spec = behavioral contract), the useful abstraction is:
- **What** not **how**: intent, not implementation
- Comments in real codebases split into six intent categories (Sun et al.): *what*, *why*, *how-to-use*, *how-it-is-done*, *property*, *other*. Spec generation needs *what* + *property* types; current LLM summarizers conflate all six.
- Larger models over-specify: DeepSeek Coder 33B produced "overly detailed summaries" — a risk for the 10x compression goal.

**Practical implication**: function-level with class skeleton context is the sweet spot. Repository-level is needed only for cross-cutting specs. Summaries need post-processing to strip *how-it-is-done* content.

---

## 2. Behavioral Equivalence Verification

### Theoretical status

Program equivalence is undecidable in the general case (Rice's theorem). This applies regardless of whether an LLM is involved. For SQL specifically, query equivalence is also undecidable. No practical system escapes this — the question is which approximation to use.

### Layered pragmatic approach (Berkeley / Cheung 2025)

The emerging consensus is a **layered verification stack**, applied cheapest-first:
1. Syntax check (does it compile/parse?)
2. Type check (do types match?)
3. Contracts / pre-post conditions
4. Property-based tests (randomized inputs, invariant checks)
5. Security rule checks
6. Bounded formal proofs (limited depth/input space)

Each layer catches different classes of divergence. Full formal verification (step 6) is expensive and has scalability limits; property-based testing (step 4) is the practical workhorse.

### EquiBench (Feb 2025)

Benchmark for LLM code equivalence reasoning. Key finding: LLMs excel at **surface-level pattern matching** (renaming variables, reformatting) but fail when deeper semantic analysis is required — nested conditionals, loops, context-sensitive behavior. State-of-the-art LLMs cannot reliably determine behavioral equivalence; they approximate via heuristic similarity.

### Mutation testing as proxy

Mutation score is the most established empirical proxy for behavioral equivalence in testing:
- Equivalent mutants (semantically identical variants that survive tests) remain the biggest practical obstacle — detecting them requires additional analysis
- **State field coverage** (dynamic oracle quality metric) strongly correlates with mutation score — useful for assessing how well a test suite covers behavioral differences
- **Semantic-preserving transformations study (2025)**: applying 16 safe transformations to code changed LLM vulnerability-detection predictions in **49% of cases** (split ~25% false positive / 24% false negative). This reveals that LLMs are not semantically invariant to code presentation — a critical finding for equivalence proxies.

### Testora (March 2025) — NL oracle for regression detection

Testora uses PR metadata (title, description, commit messages) as a **natural language oracle** for regression detection:
1. Generate 20 tests targeting modified functions
2. Run both old and new code in Docker containers, compare outputs
3. LLM classifier: does the behavioral difference align with stated PR intent?

Results: 19 confirmed regressions + 11 bug fixes detected across keras/marshmallow/pandas/scipy. 58% precision, 60% recall. Cost: $0.003 and 12.3 minutes per PR.

**Directly applicable to Code Takeover**: this is the verification approach CodeSpeak needs — use spec edit intent as oracle to classify whether code changes are expected or regressions.

### SpecGen (2024) — formal spec generation

SpecGen generates JML (Java) pre/post/invariant specifications via two phases:
- Phase 1: LLM conversation with error feedback (up to 10 rounds)
- Phase 2: mutation operators on failed specs (comparative ≤→<, logical &&→||, predicative ∀→∃, arithmetic +→−)

Results: 100/120 programs succeeded vs. Daikon (21/120) and Houdini (42/120). Human rating: 4.54/5.0 (oracle: 4.83). This is the state of art for formal spec extraction from code.

**Limitation**: JML specs are formal, not the "concise NL" CodeSpeak targets. The mutation-based repair loop is transferable.

---

## 3. Roundtrip Consistency (Code→Spec→Code)

### Model-Driven Engineering lessons (UML roundtripping)

Round-trip engineering (RTE) in MDE: keep models and code synchronized without information loss. The lesson from 20+ years of UML RTE is:

- **Perfect roundtripping is a myth**: "A common problem in RTE tools is that the model reversed is not the same as the original one, unless the tools are aided by leaving laborious annotations in the source code."
- **Bidirectional transformations** (QVT-R standard) exist but are "rarely used in model-driven software development" despite modelers needing them — the gap between theory and practice is wide.
- UML→Java has well-understood limitations: associations and containment have no direct Java equivalent, so reverse-engineering them is lossy.
- **The standard outcome**: RTE tools achieve partial roundtripping for the "clean" subset of code that maps neatly to model concepts, and fall back to manual annotation for the rest.

**Direct parallel to CodeSpeak**: code has "essential" vs. "boilerplate" regions. Boilerplate roundtrips well (spec↔code is invertible). Essential logic that encodes domain knowledge does not roundtrip cleanly because the spec must capture *intent*, which is not mechanically derivable from code.

### Clover (Stanford, 2024) — closed-loop consistency triangle

Clover checks consistency among **three artifacts**: code, formal annotations, docstring. Six directed edges:

| Edge | Mechanism |
|------|-----------|
| code → annotation | Dafny formal verifier (sound proof) |
| annotation → code | LLM regenerates code; functional equivalence tested |
| annotation → docstring | LLM generates docstring; semantic equivalence via LLM judge |
| docstring → annotation | LLM generates annotation; logical equivalence checked formally |
| code → docstring | LLM generates docstring; semantic equivalence via LLM judge |
| docstring → code | LLM generates code; functional equivalence tested |

Results on CloverBench: **87% acceptance rate** for correct instances, **0% false positive rate** for adversarially incorrect ones (k=10 runs). Also discovered 6 incorrect programs in existing human-written dataset MBPP-DFY-50.

**Key lesson**: the triangle structure catches inconsistencies that single-direction verification misses. NL docstring → formal annotation → code verification is more robust than direct NL → code.

### What CodeSpeak's blog reveals

From the "Code Takeover" blog post (Feb 2026): CodeSpeak generates `.cs.md` spec files from source. Verification is via **test vectors** — generated inputs that check specific output properties. The system acknowledges planned but not-yet-implemented: "verifying that, when editing the spec, we can generate adequate changes in the code (spec diff → code diff)." This confirms the roundtrip verification gap is open.

The system uses `codespeak.json` as a registry mapping specs to code files. The takedown of the old code is conditional on tests passing.

### Lossy vs. lossless in practice

All code→spec→code roundtrips are **lossy by design** — the point is to compress. The question is: lossy in which dimensions?

| Dimension | Lossy? | Recoverable? |
|-----------|--------|--------------|
| Variable names / formatting | Yes | No (and intentional) |
| Algorithm choice | Yes | LLM picks valid alternative |
| Performance characteristics | Yes | Not in behavioral spec |
| Functional behavior | Should be preserved | Verifiable via tests |
| Edge case handling | Often lost | Critical gap |
| Error messages / UX text | Often lost | Need explicit spec |

The "equivalent mutant" problem from mutation testing reappears: two implementations can be observationally equivalent on all tests but diverge on untested edge cases. The spec cannot prevent this without exhaustive test coverage.

---

## 4. Why Intermediate Representations Fail

### The core empirical finding

**"Can LLMs Understand Intermediate Representations?" (Feb 2025)** — tested LLVM IR at -O0 through -O3 vs. source code:

| Model | IR pass rate | Source code pass rate | Degradation |
|-------|-------------|----------------------|-------------|
| GPT-4 | 36% | 72% | 2× |
| GPT-3 | 4% | 15.8% | 4× |
| Gemma 2 | 19.5% | 61% | 3× |
| LLaMA 3.1 | 18.9% | 73% | 4× |
| Code Llama | 27% | 80% | 3× |

IR consistently produces 2–4× performance degradation across all models and tasks.

### Why IR fails — three structural reasons

1. **Loss of high-level semantics**: IR strips variable names and conceptual groupings. LLMs are trained on source code with meaningful identifiers; IR is a stripped-down, register-based form that lacks the cues models rely on.

2. **Verbosity and context explosion**: at -O0, LLVM IR exceeds context windows for smaller models. GPT-4 had 3 failures from length; Code Llama had 99 failures.

3. **Control flow complexity**: LLMs misinterpret branching instructions (br, jmp). CFG reconstruction accuracy remains low even for GPT-4 (39/164 fully correct CFGs despite attempting all 164).

### The broader principle: IR as information degradation

The "IR makes things worse" finding generalizes beyond LLVM:

- **Entity extraction as IR**: extracting entities (classes, functions, call graphs) from code produces a sparse representation that drops implementation context, inter-function dependencies, and behavioral semantics. The LLM then reasons on an impoverished signal.
- **AST as IR**: ASTs preserve structure but not semantics. They are useful for syntax-level tasks; for behavioral understanding, they drop the "why" entirely.
- **The decomposition trap**: "Can LLMs Replace Humans During Code Chunking?" (2025) finds that arbitrary decomposition breaks cross-chunk dependencies — the aggregation step cannot recover lost global context.

### Chain-of-thought vs. direct prompting for code

**Structured CoT (SCoT)** — using program structures as intermediate steps — improves *code generation* (HumanEval +16%, MBPP +17%) but the picture reverses for *code summarization*:
- For summarization, "advanced prompting techniques may not outperform simple zero-shot prompting"
- CoT increases token consumption 2–4× with inconsistent quality gains
- The key variable is task complexity: CoT helps when the task requires multi-step reasoning; it hurts when the task is pattern recognition where the model already has the answer without explicit steps

**Implication for Code Takeover**: extracting entities as intermediate steps before spec generation is likely harmful. The spec should be generated holistically from the source, not assembled from entity fragments.

### Why Breslav's intuition is correct

The research validates the claim that IR often produces worse results:
- LLMs are trained on natural language and high-level code; their competence degrades rapidly as representation departs from training distribution
- Intermediate decomposition introduces two error sources (decomposition error + synthesis error) vs. one (direct generation error)
- Entity extraction throws away the behavioral context that makes code meaningful
- The Clover triangle works precisely because it uses *natural language docstrings* (high-level, close to training distribution) as the bridge — not ASTs or entity graphs

---

## 5. Practical Approaches

### Test suite as equivalence oracle

The most practical and widely validated approach. Key principles:

**Regression oracle**: run existing test suite against regenerated code. If tests pass, behavioral equivalence is claimed within test coverage. Limitation: regression oracles assume the existing test suite is correct and complete — they reinforce bugs rather than detect them.

**Differential testing**: run both old implementation and spec-regenerated implementation on identical inputs; flag divergences. The "pseudo-oracle" approach — no ground truth needed, discrepancies are automatically detectable.

**Testora's approach** (applicable directly): generate additional tests targeting modified regions using LLM + PR metadata. This addresses the test coverage gap without requiring formal specs.

**CLEVER benchmark finding (2025)**: testing recognizes >40% of LLM-generated specs as sound and complete; formal proof succeeds in <4% of cases. Testing is 10× more tractable than formal verification for current LLMs.

### Spec quality metrics

| Metric | What it measures | Reliability |
|--------|-----------------|-------------|
| BLEU/ROUGE-L | Lexical similarity to reference | Poor — reference quality is often low |
| BERTScore | Semantic similarity | Better for NL alignment |
| BLEURT | Learned human evaluation proxy | Strong for code summarization |
| GPT-4 as judge | Alignment with human ratings | Best (0.28–0.65 correlation) |
| Mutation score | Behavioral coverage of tests | Gold standard for oracle quality |
| State field coverage | Dynamic oracle quality | Correlates with mutation score |
| Checked coverage | Whether test assertions actually inspect executed code | Under-used but valuable |

For spec-from-code, **GPT-4 as judge + mutation score** on a downstream test suite is the most meaningful combined metric.

### Progressive conversion strategy

"Migrating Code at Scale with LLMs" (Google, 2025) and related work converges on:

1. **Incremental, not big-bang**: convert one module at a time; keep old code running during transition
2. **Compatibility shim**: spec-managed and legacy code coexist; interfaces are the boundary
3. **Test first**: build or expand the test suite before converting — tests are the equivalence oracle
4. **High-confidence regions first**: functions with full test coverage convert safely; partial-coverage functions need test expansion
5. **Progressive trust**: start with low-risk (utility functions), end with high-risk (core business logic)

### Diff-based verification (spec diff → code diff)

The unsolved problem CodeSpeak acknowledges: given a spec change δS, verify that the resulting code change δC is adequate.

**Closest research**: Testora's NL oracle — classify whether behavioral changes align with the stated change intent. Applicable directly: when a spec edit is made, the system should:
1. Generate tests targeting the edited region
2. Run before/after code versions
3. Classify: does the behavioral change match the spec delta?

### SpecGen's mutation-repair loop as spec validation

When spec→code regeneration fails verification, mutation operators (comparative, logical, predicative, arithmetic) can systematically explore nearby specs. This is a **repair oracle**, not just a correctness checker — it provides a path from "spec that fails verification" to "spec that passes".

### Avoiding over-specification

The "Beyond Basic Specifications" study (2025) found:
- Axiom-based specs are the most unstable (19.76% verification reduction under deletion)
- No single syntactic construct class achieves complete coverage — different programs need different spec styles
- Under-specification is safer than over-specification for regeneration: a tighter spec reduces generation freedom but increases verification tractability

**Practical rule**: specs should be as weak as possible while still excluding wrong implementations. Strengthening happens when test failures expose gaps.

---

## Key Tensions and Open Problems

| Problem | Current Best Approach | Gap |
|---------|----------------------|-----|
| Undecidability of equivalence | Layered testing + bounded proofs | Cannot prove equivalence for all inputs |
| Spec completeness | Test-driven gap detection | Unknown unknowns — tests don't cover what they don't cover |
| IR makes things worse | Direct NL spec generation | Need mechanism for structured specs without IR loss |
| Roundtrip lossy in edge cases | Explicit edge case test vectors | Manual effort to enumerate corners |
| Spec ambiguity | LLM clarification loops | Ambiguity detection is unsolved |
| Incremental edits | Delta testing (Testora approach) | Spec diff → code diff mapping is unsolved |

---

## Sources

- [Source Code Summarization in the Era of Large Language Models (ICSE 2025)](https://arxiv.org/html/2407.07959v1)
- [Code Summarization Beyond Function Level — Makharev et al. (Feb 2025)](https://arxiv.org/html/2502.16704v1)
- [P2N2S: Bridging NL and PL for Code Summarization — Springer 2025](https://link.springer.com/article/10.1007/s11280-025-01395-3)
- [EquiBench: Benchmarking LLMs on Code Equivalence Reasoning (Feb 2025)](https://www.arxiv.org/pdf/2502.12466)
- [Can Large Language Models Understand Intermediate Representations? (Feb 2025)](https://arxiv.org/abs/2502.06854)
- [Clover: Closed-Loop Verifiable Code Generation — Stanford / POPL 2024](https://arxiv.org/abs/2310.17807) · [Blog](http://ai.stanford.edu/blog/clover/)
- [SpecGen: Automated Generation of Formal Program Specifications via LLMs (2024)](https://arxiv.org/html/2401.08807v1)
- [Testora: Regression Detection with Natural Language Oracle (March 2025)](https://arxiv.org/html/2503.18597v1)
- [Semantic-Preserving Transformations as Mutation Operators (March 2025)](https://arxiv.org/html/2503.23448v1)
- [Beyond Basic Specifications: Logical Constructs in LLM-based Spec Generation (2025)](https://arxiv.org/html/2602.00715v1)
- [Test Oracle Automation in the Era of LLMs — ACM TOSEM (2025)](https://dl.acm.org/doi/10.1145/3715107)
- [Round-trip Engineering — Wikipedia](https://en.wikipedia.org/wiki/Round-trip_engineering)
- [Systematic Comparison of Roundtrip Software Engineering Approaches — ScienceDirect](https://www.sciencedirect.com/science/article/pii/S1877050921002830)
- [CodeSpeak Takeover Blog Post (Feb 2026)](https://codespeak.dev/blog/codespeak-takeover-20260223)
- [The Programming Language After Kotlin — Pragmatic Engineer](https://newsletter.pragmaticengineer.com/p/the-programming-language-after-kotlin)
- [CLEVER: Curated Benchmark for Formally Verified Code Generation (2025)](https://arxiv.org/pdf/2505.13938)
- [Migrating Code at Scale with LLMs at Google (2025)](https://arxiv.org/html/2504.09691v1)
- [State Field Coverage: A Metric for Oracle Quality (2025)](https://arxiv.org/html/2510.03071v1)
- [Checked Coverage: An Indicator for Oracle Quality](https://www.researchgate.net/publication/263452357_Checked_coverage_An_indicator_for_oracle_quality)
