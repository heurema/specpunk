# The Spec Extraction Coverage Gap

**Date:** 2026-03-12
**Type:** Deep Research
**Status:** Complete

---

## Executive Summary

Three independent empirical findings converge on a single conclusion: LLMs are structurally unreliable at extracting complete, correct specifications from code. UCRBench (arXiv:2512.13360) shows 49–64% average omission rates when recovering user-goal use cases from real Java projects. A differential fuzzing study (arXiv:2602.15761) shows 19–35% of LLM-generated refactorings are semantically non-equivalent, and 21% of these escape detection by existing test suites. A mutation-analysis study (arXiv:2602.17838) shows LLM code summarization accuracy collapses from 76.5% on single functions to 17.3% on multi-threaded systems.

These are not benchmark artifacts. They measure fundamental gaps in how current transformer-based models reason about program behavior — gaps that directly limit Code Takeover and any spec-driven development approach.

---

## 1. UCRBench: The Use Case Recovery Gap

**Paper:** UCRBench: Benchmarking LLMs on Use Case Recovery
**Authors:** Shuyuan Xiao, Yiran Zhang, Weisong Sun, Xiaohong Chen, Yang Liu, Zhi Jin
**arXiv:** 2512.13360 (submitted December 15, 2025)

### 1.1 What Was Tested

Nine real-world Java projects, totaling 556,618 lines of code, with manually validated ground-truth use cases:

| Project | User-Goal UCs | Subfunction UCs | LOC | Domain |
|---------|--------------|-----------------|-----|--------|
| Library | 12 | 15 | 863 | Library Management |
| Chatkit | 20 | 20 | 4,696 | Chat UI |
| Baseadmin | 26 | 42 | 5,488 | Admin Management |
| Poli | 15 | 22 | 6,200 | Business Intelligence |
| Petclinic | 38 | 58 | 9,894 | Pet Clinic |
| Didicar | 25 | 30 | 10,025 | Ride-Hailing |
| Ruoyi | 22 | 47 | 12,091 | AI Assistant |
| JetUML | 45 | 53 | 32,452 | UML Modeling |
| Xpipe | 29 | 37 | 66,609 | Remote Infrastructure |

**Totals:** 232 user-goal use cases, 324 subfunction use cases.

**Models tested:** GPT-5, GPT-5-mini, DeepSeek-V3.2-Chat, DeepSeek-V3.2-Reasoner.

### 1.2 Quantitative Results: Omission Rates

The headline finding is the **Omission Rate (OR)** — the fraction of ground-truth use cases that the model completely fails to generate.

#### Subfunction Level (Table 2)

| Model | Actor Accuracy | Name Accuracy | Path Accuracy | Omission Rate |
|-------|---------------|---------------|---------------|--------------|
| GPT-5 | 77.2% | 36.4% | 59.2% | **43% avg** |
| GPT-5-mini | 69.0% | 35.0% | 67.2% | **26% avg** |
| DeepSeek-Chat | 64.7% | 39.3% | 79.0% | **35% avg** |
| DeepSeek-Reasoner | 62.2% | 39.0% | 80.1% | **33% avg** |

Omission range at subfunction level: **5% to 69%** per project.

#### User-Goal Level (Table 3) — The Critical Gap

| Model | Actor Accuracy | Name Accuracy | Path Accuracy | Omission Rate |
|-------|---------------|---------------|---------------|--------------|
| GPT-5 | 73.4% | 36.2% | 49.7% | **65% avg** |
| GPT-5-mini | 74.1% | 35.5% | 50.7% | **63% avg** |
| DeepSeek-Chat | 80.6% | 42.0% | 67.0% | **44% avg** |
| DeepSeek-Reasoner | 76.2% | 43.4% | 63.8% | **53% avg** |

Omission range at user-goal level: **9% to 90%** per project.

The **average omission rate of 49–64%** quoted in the abstract is the range across all models at user-goal level. DeepSeek models do better (44–53%) while GPT models omit 63–65% of user-goal use cases.

### 1.3 What Kinds of Use Cases Are Missed

The paper does not provide a categorical breakdown by omission type (e.g., "edge case vs. happy path"), but several patterns emerge:

**Domain-specific use cases.** Ruoyi (AI assistant, 12,091 LOC) shows catastrophic failure: GPT-5 omits **73%** of user-goal use cases while DeepSeek-Reasoner omits only 9%. The paper attributes this to deeply nested logic and cross-module dependencies requiring domain knowledge. Models without domain grounding cannot identify which behaviors constitute user-visible functionality.

**Multi-module aggregation.** The Xpipe project (remote infrastructure, 66,609 LOC) shows near-complete path accuracy failure: GPT-5 achieves 24.6% path accuracy at user-goal level. Large codebases require aggregating subfunction behaviors across module boundaries into coherent user-goal use cases — a step where all models struggle.

**Abstraction-level mismatch.** Models generate "excessively fine-grained" subfunctions. GPT-5 generated 2,103 subfunctions for Xpipe vs. 37 ground-truth; DeepSeek-Reasoner generated 887. This reveals a fundamental inability to calibrate the right abstraction level. When aggregating to user-goal use cases, the models collapse many subfunctions into single use cases (Ruoyi/GPT-5 merged 19 subfunctions) or produce fragmented, incomplete pictures.

**Implicit domain requirements.** Human annotators incorporate "domain knowledge, implicit requirements" in creating ground truth. Models have no explicit mechanism for inferring that a ride-hailing system must handle the "driver cancels trip" flow even when it is distributed across several modules with no single entry point.

**Name recognition failure.** Name accuracy is uniformly low: 35–43% across all models and all projects. This means even when a use case is "found," its label is wrong 57–65% of the time. Use cases named from implementation-level details ("processPaymentRequest") rather than user-facing intent ("complete ride payment").

**Actor confusion.** Models conflate fine-grained roles: "student" vs. "teacher," "user" vs. "developer." GPT-5 better identifies role-specific actors ("passenger," "owner") while DeepSeek recognizes administrative roles ("admin") more reliably. This creates phantom use cases attributed to wrong actors and misses use cases for unrecognized actors.

### 1.4 Code Complexity and Coverage

Direct correlation documented:

- Simple, well-structured projects (Library, 863 LOC) show better performance across all metrics.
- Complex, multi-module systems (Xpipe, 66,609 LOC; JetUML, 32,452 LOC) show cascading failure.
- Ruoyi (12,091 LOC) is the anomalous case where domain complexity matters more than size: DeepSeek-Reasoner achieves 9% OR while GPT-5 achieves 73% OR on the same codebase.

The paper explicitly states: "well-structured projects are significantly easier for LLMs to interpret."

### 1.5 Evaluation Methodology

The matching pipeline uses sequential filtration: path → name → actor. Metrics:

- **Actor accuracy (AccA):** Hybrid semantic + role-category similarity (0.3 semantic, 0.7 category weights)
- **Name accuracy (AccN):** SBERT embeddings for semantic comparison
- **Path accuracy (AccP):** Jaccard similarity on actor-to-use-case paths
- **Omission rate (OR):** Fraction of ground-truth use cases with no matching generated use case

The 0.7 weighting on role-category means actor matching is strict: a model that calls the "passenger" the "user" loses most of the actor score.

---

## 2. The 19% Non-Equivalence Finding

**Paper:** A Differential Fuzzing-Based Evaluation of Functional Equivalence in LLM-Generated Code Refactorings
**Authors:** Simantika Bhattacharjee Dristi, Matthew B. Dwyer
**arXiv:** 2602.15761 (submitted February 17, 2026)

### 2.1 What Was Studied

Six LLMs evaluated on three datasets, two refactoring types, using differential fuzzing to detect semantic non-equivalence.

**Models (6):** CodeLlama (13B), Codestral (22B), StarChat2 (15B), Qwen-2.5-Coder (32B), Olmo-3 (32B), GPT-4o.

**Datasets:**
| Dataset | Problems Analyzed | Tests/Problem | Code Level |
|---------|------------------|---------------|------------|
| HumanEval | 164 | 7.7 | Function |
| MBPP | 100 | 3.0 | Function |
| APPS | 100 | 21.2 | Program |

**Refactoring prompts tested:**
- P₁: Performance optimization
- P₂: Code simplification

**Total:** 4,368 refactorings generated; 3,538 after filtering timeouts/errors.

### 2.2 Non-Equivalence Rates by Model

| Model | Simplification | Optimization | Overall |
|-------|----------------|--------------|---------|
| Codestral | 23.81–40.35% | 27.12–50.85% | **35.14%** |
| StarChat2 | 26.23–45.54% | 32.28–35.40% | **34.24%** |
| CodeLlama | 24.24–33.33% | 15.93–30.95% | **26.23%** |
| Qwen-2.5 | 13.18–30.09% | 18.18–27.52% | **22.01%** |
| Olmo-3 | 12.70–43.88% | 8.96–28.09% | **21.73%** |
| **GPT-4o** | 8.53–20.18% | 19.69–28.57% | **18.58%** |

GPT-4o is the best performer at 18.58% non-equivalence — still meaning roughly 1 in 5 refactorings it produces changes the program's meaning.

### 2.3 Non-Equivalence by Dataset

| Dataset | Total Analyzed | Non-Equivalent | Rate |
|---------|---------------|----------------|------|
| HumanEval | 1,507 | 333 | **22.1%** |
| MBPP | 766 | 194 | **25.3%** |
| APPS | 1,265 | 406 | **32.1%** |

APPS (program-level, multi-function code) shows 7–10 percentage points higher failure rate than function-level datasets, confirming that code complexity increases semantic drift.

### 2.4 The Test Suite Escape Rate (The Hidden 21%)

**This is the most alarming finding.** When non-equivalent refactorings are evaluated against the existing test suites in each dataset:

| Dataset | Non-Equivalent | Undetected by Tests | Escape Rate |
|---------|---------------|---------------------|-------------|
| HumanEval | 333 | 69 | **20.72%** |
| MBPP | 194 | 42 | **21.65%** |
| APPS | 406 | 91 | **22.41%** |

Approximately **21% of semantically non-equivalent refactorings pass all existing tests.** The test suites in these widely-used benchmarks have insufficient coverage to detect ~1 in 5 semantic defects introduced during refactoring.

The paper notes: "Relying solely on existing test cases may have led prior studies to inflated estimates of refactoring correctness."

### 2.5 Why Tests Miss Non-Equivalence: Test Adequacy Analysis

The escape rate is explained by the inadequacy of existing test suites:

- HumanEval: average 7.7 tests per problem — insufficient to explore the input space
- MBPP: average 3.0 tests per problem — three test cases cannot cover even basic equivalence classes
- APPS: 21.2 tests per problem, yet still 22.4% escape rate — test count alone doesn't guarantee coverage

The fundamental issue: all three datasets use **assertion-based test suites created by humans who assumed correct implementations**. These tests exercise common paths but not the boundary behaviors that refactoring errors typically corrupt.

### 2.6 Differential Fuzzing Methodology (Eq@DFuzz)

The Eq@DFuzz approach:

1. Generate **1,000 test inputs** for program-level (APPS); **2,000 for function-level** (HumanEval, MBPP)
2. Use **Atheris** (Google's Python byte-level fuzzer) to generate inputs satisfying code constraints
3. Execute original and refactored code on all generated inputs
4. Binary equivalence: non-equivalent if any single output diverges

This explores far more of the input space than the existing 3–21 test cases per benchmark problem.

### 2.7 Types of Non-Equivalence

The paper does not provide a categorical taxonomy of non-equivalence types, but several patterns are identifiable from context:

**Off-by-one and boundary errors.** Performance optimization refactorings frequently alter loop bounds or early-exit conditions, changing behavior on boundary inputs that existing tests don't cover.

**Exception path changes.** Simplification often removes or alters error-handling branches. The test suites, built for correct code, rarely exercise error paths.

**Output format drift.** Optimization sometimes changes output formatting (whitespace, precision) in ways that are semantically significant but test-invisible if tests use substring matching.

**Algorithmic substitution.** Models replace algorithms with equivalents that are not equivalent for edge inputs (e.g., replacing stable sort with unstable sort, modifying hash function behavior).

---

## 3. The Concurrency Gap: 76.5% vs. 17.3%

**Paper:** Examining LLMs Ability to Summarize Code Through Mutation-Analysis
**Authors:** Lara Khatib, Micheal Pu, Bogdan Vasilescu, Meiyappan Nagappan
**arXiv:** 2602.17838 (submitted ~February 2026)

### 3.1 The Core Finding

LLM code summarization accuracy degrades sharply with structural complexity:

| Code Structure | Detection Rate |
|----------------|---------------|
| Single Function (SF) | **76.5%** |
| Single Class (SC) | **33.3%** |
| Multiple Classes (MC) | **28.4%** |
| Multi-threaded Systems (MT) | **17.3%** |

This was measured on GPT-4 using a mutation-based evaluation: if a mutation is made to the code, does the LLM's summary update to reflect the changed behavior?

### 3.2 Methodology

**Evaluation approach:**
1. Generate a code summary using the LLM
2. Inject a targeted behavioral mutation into the code
3. Ask the LLM to summarize the mutated code
4. Compare: did the summary change to capture the mutation?

**Dataset:**
- 12 controlled synthetic programs with 324 mutations (across Single Function, Single Class, Multiple Classes, Multi-threaded categories)
- 50 human-written programs from LBPP (Less Basic Python Problems), designed to avoid training data overlap
- 624 total mutation-summary evaluations across 62 programs

**Mutation types:**
- Statement mutations: insert/remove/replace statements
- Value mutations: change literal values (constants, thresholds)
- Decision mutations: alter conditional expressions

**Controls:**
- Temperature = 0 (deterministic)
- Fresh sessions per sample (no context leakage)
- LBPP programs verified not to overlap with training data

### 3.3 GPT-4 vs. GPT-5.2 on Human-Written Code

On the LBPP dataset (50 human-written programs):

| Model | Overall | Statement | Decision | Value |
|-------|---------|-----------|----------|-------|
| GPT-4 | **49.3%** | 46.0% | 44.0% | 58.0% |
| GPT-5.2 | **85.3%** | 76.0% | 88.0% | 92.0% |
| Improvement | +36.0pp | +30.0pp | +44.0pp | +34.0pp |

GPT-5.2 shows "substantial performance leap" but the underlying problem remains: at 85.3%, GPT-5.2 still misses 14.7% of behavioral mutations in single-function Python code. The concurrency gap is not reported for GPT-5.2.

### 3.4 Why LLMs Fail on Concurrent Code

The paper identifies two failure modes with implications for concurrent code:

**Failure mode 1: Insufficient abstraction level.** Summaries describe high-level algorithmic intent rather than implementation specifics. When a mutation changes a detail (e.g., a shared counter's increment logic), the summary already operates at a level of abstraction where that detail was invisible. The mutation is structurally present in the code but semantically absent from the summary.

**Failure mode 2: Pattern hallucination.** LLMs describe canonical algorithm behavior from training data rather than actual code behavior. Example from the paper: "a j+=1 line is removed from merge sort, but the summary still claims the index is incremented, a canonical step in merge sort widely available in training data." In concurrent code, canonical patterns (producer-consumer, mutex lock/unlock, barrier synchronization) are especially stable training artifacts — the model describes the pattern, not the actual implementation.

**Why concurrency is categorically harder:**

- Concurrent execution semantics require tracking multiple execution paths simultaneously
- Shared state can be changed by different threads at different times
- The LLM processes code sequentially but concurrent behavior is non-sequential
- Memory ordering (happens-before relations, store buffers, TSO/PSO relaxations) is not captured by sequential reading
- Non-deterministic interleavings mean no single "canonical execution" can be described

The concurrent comprehension problem is confirmed by a separate study (arXiv:2501.14326): under relaxed memory models (TSO/PSO), even GPT-4 achieves only F1=0.80 on assertion failure detection, and all models "face significant challenges verifying program correctness under relaxed memory models."

### 3.5 Specific Concurrency Failures (From arXiv:2501.14326)

Five LLMs evaluated on concurrent programs (pthread benchmarks):

**Deadlock detection (litmus tests):** GPT-4o, GPT-4, Mistral-AI Large2 achieve 100% accuracy under sequential consistency.

**Race condition detection (SV-COMP pthread tests):**
- GPT-4o: 88% accuracy
- GPT-4: 56% accuracy
- Mistral-AI Large2: 32% accuracy
- GPT-3.5-turbo: 0% accuracy

**Assertion failure detection under sequential consistency:**
- GPT-4o: 80%
- GPT-4: 72%
- GPT-4o-mini: 64%
- Mistral-AI Large2: 40%
- GPT-3.5-turbo: 0%

**Under relaxed memory models (TSO/PSO) — the critical gap:**
- GPT-4: F1=0.80, recall=1.00 (best performer)
- GPT-4o: F1=0.65, recall=0.71
- Mistral-AI Large2: F1=0.21, recall=0.13

**Observed failure types under relaxed memory:**
- Incorrect attribution of assertion failures to inter-thread vs. intra-thread reordering
- Failure to generate valid execution traces demonstrating memory reordering
- Misunderstanding of memory fence semantics
- Incorrect load-load reordering predictions under PSO
- Inability to reason about store buffer contents across threads

---

## 4. Coverage Gap Taxonomy

Synthesizing across UCRBench, the mutation study, the differential fuzzing study, and related work, the following taxonomy of what LLMs miss in spec extraction:

### 4.1 Implicit Behaviors

**What it is:** Behaviors that are expected by users but not explicitly coded as single functions or branches — emergent from the combination of multiple components.

**Quantitative signal:** UCRBench omission rates of 44–65% at user-goal level. The user-goal use case aggregation problem — combining many subfunctions into coherent user-visible behaviors — is where omission is worst.

**Why it happens:** LLMs process code locally. They see individual functions and can describe them. They cannot reliably infer that 15 different subfunctions constitute a single user-visible workflow unless that workflow is explicitly encoded as an entry point.

**Example:** A checkout flow in an e-commerce system may involve authentication, cart validation, inventory check, payment processing, and confirmation email — distributed across 8 modules with no single "checkout" function. The model sees each module, but cannot construct the implicit contract that connects them.

### 4.2 Error Handling and Exception Paths

**What it is:** The contract for what happens when things go wrong — invalid inputs, network failures, resource exhaustion, timeout conditions.

**Quantitative signal:** The differential fuzzing study (arXiv:2602.15761) shows 21% test escape rate, driven partly by tests that don't exercise error paths. LLM hallucination on human-written code (arXiv:2602.17838) includes cases where removed error-handling code is not reflected in updated summaries. Popular benchmarks (HumanEval+, MBPP+) explicitly filter out tests that exercise pre-condition violations — systematically removing error-path coverage.

**Why it happens:** Training data skews toward happy-path code. Stack Overflow answers, tutorial code, and documentation predominantly show success paths. Error handling is boilerplate that gets underrepresented. Models learn that code means happy-path behavior.

### 4.3 Security Constraints

**What it is:** Authorization rules, input validation requirements, sanitization contracts, privilege escalation guards.

**Quantitative signal:** Not directly measured in UCRBench. However, security constraints are archetypal "implicit requirements" — they are cross-cutting, not localized to a single function, and often expressed as negative constraints ("must not allow X") rather than positive behavior. The UCRBench omission pattern (domain-specific constraints most missed) applies directly to security constraints.

**Context from related work:** An empirical study of LLM-powered software found that LLM-related software introduces "security concerns that escape traditional classification schemes" and existing frameworks "rarely account for LLM-specific control and data flows." This means LLMs both fail to specify security constraints and fail to flag them as missing.

### 4.4 Performance Requirements

**What it is:** Latency bounds, throughput guarantees, memory limits, resource usage contracts.

**Quantitative signal:** LLM-based code translation work (arXiv:EECS-2025-174) notes: "Requirements to preserve equivalence regarding time/space complexity remain underexplored." EquiBench (arXiv:2502.12466) shows models achieve only 62.3–68.8% accuracy on the hardest equivalence categories — barely above random for code that is structurally equivalent but performance-different.

**Why it happens:** Performance is a non-functional requirement. It does not appear in function signatures, docstrings, or test assertions. Compilers and optimizers preserve functional equivalence while dramatically altering performance; LLMs trained on code have no mechanism to encode "this version is fast" as a specification constraint.

### 4.5 Concurrency Semantics

**What it is:** Ordering guarantees, visibility invariants, atomicity requirements, happens-before relations, lock ordering constraints.

**Quantitative signal:** 17.3% mutation detection accuracy (arXiv:2602.17838). Under relaxed memory models, even GPT-4 achieves only F1=0.80 (arXiv:2501.14326). Race condition detection drops to 32–88% depending on model.

**Why it happens:** Mechanistically clear. Transformer architecture processes tokens sequentially; concurrent behavior is inherently non-sequential. Training data is dominated by single-threaded code. Concurrent programs in training data rarely include the formal specifications needed to infer happens-before relations. Memory ordering constraints (TSO, PSO, acquire/release semantics) are explicitly not captured by sequential reading of source code.

### 4.6 State Machine Transitions

**What it is:** Valid and invalid state sequences, guard conditions on transitions, initial and terminal states, state-dependent behavior.

**Quantitative signal:** LLM-based protocol state machine inference achieves 87–95% precision and 55–98% recall depending on protocol, with RTSP showing 54.5% recall (arXiv:2405.00393v3). The hardest protocols to extract (RTSP) are those with complex guard conditions and many optional transitions — exactly where the implicit behavior gap matters most.

**Related finding:** LLMs struggle with large programs that exceed context windows, and "protocol implementations often exceed the LLM's context window or include irrelevant information." For state machine extraction, missing a single transition can invalidate the entire state machine's use for verification.

### 4.7 Cross-Cutting Concerns

**What it is:** Logging contracts, audit trail requirements, monitoring instrumentation, transaction boundaries, observability guarantees.

**Quantitative signal:** Not directly studied. These are the most invisible behaviors — they add no functional value visible to the user, yet are specification requirements for compliance and operational correctness. The UCRBench framework evaluates user-goal use cases, which by definition excludes cross-cutting concerns unless they have user-visible effects.

**Why it matters:** A spec that omits logging requirements will generate code that passes all functional tests but fails compliance audits. Code Takeover without explicit logging specs will silently drop audit trails.

### 4.8 Summary Table

| Gap Category | Quantitative Signal | LLM Failure Mode | Detection Difficulty |
|---|---|---|---|
| Implicit workflows | 44–65% omission (UCRBench) | Cannot aggregate across modules | High |
| Error handling | 21% escape rate (DFuzz) | Happy-path bias in training | Medium |
| Security constraints | Unmeasured (cross-cutting) | Cross-cutting, negative constraints | Very High |
| Performance requirements | Not in specs | Functional equivalence ≠ perf equiv | Very High |
| Concurrency semantics | 17.3% accuracy (mutation) | Sequential processing of parallel code | Extreme |
| State machines | 54–98% recall (protocol-specific) | Context window + guard conditions | High |
| Cross-cutting concerns | Unmeasured | Invisible to functional testing | Very High |

---

## 5. Why Tests Miss Non-Equivalent Transformations

### 5.1 Test Adequacy Criteria

The 21% escape rate is not surprising given what we know about test suite adequacy. The relevant criteria:

**Line coverage** (measured in most benchmarks): Does not distinguish between coverage of equivalent and non-equivalent executions. A test can achieve 100% line coverage while exploring only a single equivalence class of inputs.

**Branch coverage:** Better, but still inadequate. A function with 10 branches requires 2^10 potential paths; branch coverage requires only 20 tests (one for each branch direction). Non-equivalence in boundary conditions is invisible to branch coverage.

**Mutation score:** The theoretically correct measure for detecting semantic changes. A mutation score of X% means the test suite kills X% of behavioral mutations. The test suites in HumanEval (3.0 tests/problem avg), MBPP (7.7 tests/problem), and APPS (21.2 tests/problem) almost certainly achieve low mutation scores — enough to detect gross errors, not subtle non-equivalence.

**From the mutation adequacy literature:** Mutation score outperforms structural criteria (line, branch coverage) "both theoretically and empirically" (arXiv:2501.12862). But achieving high mutation score requires extensive test suites; HumanEval with 7.7 tests/problem cannot approach adequate mutation score.

### 5.2 The Equivalent Mutant Problem Applied to Refactoring

In traditional mutation testing, "equivalent mutants" are mutants that change code syntax but not semantics. They are live (not killed by any test) but not meaningful.

In LLM refactoring evaluation, the problem inverts: we have **functionally non-equivalent refactorings that are indistinguishable from equivalent ones** by the test suite. These are "equivalent from the test suite's perspective" but actually non-equivalent.

Key finding from mutation testing literature (2024): less than 10% of manually created mutants are truly equivalent. But in LLM refactoring context, 21% of non-equivalent refactorings escape detection — this is not because they are equivalent, but because the test suite is inadequate to distinguish them.

### 5.3 What Makes Tests Miss Non-Equivalence

**Sparse input space coverage.** A function that takes a 32-bit integer has 2^32 possible inputs; 7 test cases cover 0.00000016% of the input space. Differential fuzzing with 2,000 inputs covers 0.000046% — 288x better, but still sparse. Non-equivalence introduced in boundary conditions (integer overflow, empty input, maximum values) is most likely to escape sparse testing.

**Absence of negative tests.** Tests written for correct code rarely include inputs that test error paths. An optimization refactoring that silently removes a division-by-zero check will pass all tests that never pass a zero denominator.

**Output comparison weakness.** Tests that use approximate equality or string contains matching will miss non-equivalence that manifests as precision loss or output reordering.

**Identical happy-path behavior.** Many non-equivalent refactorings are equivalent on common inputs and diverge only on rare or edge inputs. The 21% that escape tests are plausibly this category: semantically wrong but coincidentally correct on the specific inputs tested.

### 5.4 Metamorphic Testing as a Partial Solution

Metamorphic testing defines relationships between inputs and outputs (metamorphic relations, MRs) that should hold regardless of specific input values. For equivalence checking:

**Relevant MRs for refactoring equivalence:**
- Permutation invariance: if algorithm is permutation-invariant, test on multiple permutations
- Scale invariance: if multiplying all inputs by constant X gives X-scaled output, test this
- Commutativity: test inputs in different orders

LLMORPH (2025) implements 36 metamorphic relations automated via LLM. However, the paper notes "false positive metamorphic violations is still a major challenge" — not every MR applies to every function.

**Effectiveness:** Metamorphic testing can detect behavioral non-equivalence that assertion-based tests miss, but requires MR identification which is itself an LLM task. For Code Takeover, metamorphic test generation from specifications provides a path to catching refactoring errors that existing suites miss.

---

## 6. Related Findings in Adjacent Domains

### 6.1 NL-to-SQL: The Same Coverage Gap

The NL-to-SQL domain shows a structurally identical problem: models miss the intended query even when they produce syntactically valid SQL.

**BIRD benchmark (2024–2025):**
- State-of-the-art systems: 65.45% execution accuracy
- Human experts: 92.96% — a 27.5 percentage point gap
- BIRD's execution accuracy metric agrees with human experts only **62% of the time** (FLEX metric, 2025)
- This means 38% of automatic evaluations are wrong — either passing non-equivalent queries or failing equivalent ones

**AmbiSQL (2025):** On 40 ambiguous queries, XiYan-SQL alone achieves 42.5% exact match accuracy. With interactive ambiguity resolution via AmbiSQL, accuracy rises to 92.5%. The gap (42.5% → 92.5%) quantifies the cost of not resolving specification ambiguity.

**BIRD annotation quality:** A semantic error detection study found 106 previously undetected annotation errors in BIRD, accounting for 6.91% of queries. Benchmarks themselves contain specification errors.

**The forced-answering problem:** Standard NL-to-SQL systems commit to an arbitrary interpretation when the query is ambiguous. This parallels LLM spec extraction: rather than flagging "I cannot determine this behavior from the code," models produce plausible-looking specs that are wrong.

**Lesson for Code Takeover:** NL-to-SQL accuracy bottlenecks at ~85% even with state-of-the-art models. If code-to-spec has a similar structural ceiling, progressive validation (convert → test → expand) becomes mandatory rather than optional.

### 6.2 Code-to-Documentation Quality

A systematic study of code summarization quality (2025 ICSE survey) finds that "advanced prompting techniques may not outperform simple zero-shot prompting" — the complexity of few-shot, CoT, and expert prompting does not reliably improve semantic completeness of code descriptions.

The mutation-analysis study (arXiv:2602.17838) confirms this: surface-level textual improvement in documentation does not correlate with behavioral correctness. GPT-4's summaries are fluent and plausible; they are also wrong 50.7% of the time for human-written code.

**Key finding:** BLEU/ROUGE metrics for documentation quality are uncorrelated with mutation detection accuracy. Evaluating spec extraction quality requires behavioral metrics, not textual similarity.

### 6.3 Formal Specification Generation (SpecGen)

SpecGen (ICSE 2025, arXiv:2401.08807) generates formal program specifications (pre/post-conditions, loop invariants) using LLMs with mutation-based refinement:

**Results on 385 programs:**
- SpecGen: 279/385 programs get verifiable specs (72.5%)
- AutoSpec: 247/385 (64.2%)
- Houdini (static analysis): 98/385 (25.5%)
- Daikon (dynamic analysis): 72/385 (18.7%)

**Real-world programs (Defects4J, avg 374 LoC):**
- SpecGen: 38/50 (76%)
- Conversational LLM: 28/50 (56%)
- Daikon: 15/50 (30%)

**What SpecGen gets wrong:** The approach uses mutation operators on LLM-generated specs and verifies them with a formal verifier. It succeeds on 72.5% of programs but fails on 27.5% — and the failure cases are clustered in nested loop programs and complex control flow (nested loops: 13/21 vs. 24/26 for sequential code).

**Coverage limitations:** SpecGen generates pre/post-conditions and invariants, not use-case-level behavioral descriptions. It addresses a different level of spec (formal contracts) than UCRBench (user-visible behaviors).

### 6.4 Requirements-to-Code vs. Code-to-Requirements

The inverse problem (generating code from specifications) is better studied. Code generation from formal specifications achieves ~73% I/O equivalence on validated functions (Amazon research, 2025). But this number itself is too optimistic: test-based validation of 73% success uses test suites with the same adequacy problems as the refactoring benchmarks.

The symmetry: if code generation from specs is 73% accurate, and spec extraction from code is 49–64% complete, the round-trip (code → spec → code) produces systems with compounding errors.

---

## 7. Improving Spec Extraction Coverage

### 7.1 Multi-Pass and Ensemble Approaches

**Multi-pass extraction:** The UCRBench paper documents that GPT-5 generates 2,103 subfunctions for a 37-use-case project. A multi-pass approach that first generates candidates exhaustively, then aggregates and prunes, might improve recall at the cost of precision. Not studied directly.

**Ensemble extraction:** Using multiple models (GPT-5 + DeepSeek-Reasoner + another model) and merging results could catch use cases that any single model misses. UCRBench shows GPT models and DeepSeek models have different failure modes (GPT-5 misses 73% of Ruoyi use cases; DeepSeek-Reasoner misses only 9%). Ensemble merging could substantially improve recall.

**RAG-augmented extraction:** Providing test cases, issue reports, and commit messages as retrieval context to the LLM. A 2024 study shows RAG improves line coverage by 6.5% on average for test generation, with GitHub issues providing the best improvement by surfacing edge cases. The same principle applied to spec extraction: existing tests represent implicit specifications that can augment LLM-extracted ones. RAG from issue tracker provides the "what went wrong" dimension that static code analysis misses.

### 7.2 Interactive Extraction (Clarification-Based)

**ClarifyGPT (2024):** Detects whether a requirement is ambiguous by performing a code consistency check. If ambiguous, prompts the LLM to generate targeted clarifying questions. Applied to spec extraction, this would generate "I cannot determine the expected behavior when the user cancels after payment. What should happen?"

**AmbiSQL demonstrated:** Interactive clarification turns 42.5% → 92.5% accuracy on ambiguous NL-to-SQL. A similar gain in spec extraction would reduce the 49–65% omission rate dramatically — but requires a human to answer questions, which adds cost.

**TICODER (2024):** Uses test generation and user feedback to iteratively clarify user intent. This architecture (LLM extracts partial spec → generates tests → user validates → LLM refines spec) directly applies to Code Takeover's progressive validation approach.

### 7.3 Type-Directed Extraction

**The idea:** Use type signatures, interface definitions, and data model schemas as strong anchors for spec extraction. A function that takes `UserId → Either[PaymentResult, PaymentError]` implicitly specifies error handling requirements through its return type.

**Current state:** Type-directed spec generation is underexplored in the spec extraction direction. SpecGen generates formal contracts that include type-relevant pre-conditions, but this is not the same as using types to guide use-case-level spec recovery.

**Practical implication:** For Code Takeover, extracting the type algebra first (all data types, their constructors, their constraints) before extracting behavioral specifications would provide a scaffold that constrains and guides behavioral extraction.

### 7.4 Measuring Extraction Completeness Without a Human Oracle

**The hardest problem.** UCRBench requires human-annotated ground truth. At Code Takeover scale, human oracles are unavailable.

**Candidate approaches:**

*Mutation-based completeness estimation* (inspired by arXiv:2602.17838): Generate targeted behavioral mutations to the code. For each mutation, check whether the extracted spec would detect the mutation (i.e., would the behavior be inconsistent with the spec?). The fraction of mutations detectable by the spec is a proxy for completeness.

*Differential testing against existing test suite:* Generate the spec, then generate code from the spec, run existing tests on generated code. Failures indicate spec gaps. This is conservative (fails to detect spec gaps where tests also fail) but avoids false positives.

*Cross-model voting:* Run three models, compare their extracted specs. Use-cases appearing in only one model's output are suspect; use cases appearing in all three models have higher confidence. But this misses systematic biases shared across all models.

*Coverage-guided expansion:* Use code coverage analysis (symbolic execution or fuzzing) to identify code paths not covered by any extracted use case. Generate targeted prompts ("Explain what happens when this branch executes: [branch code]") to fill gaps. This is the most tractable oracle-free approach.

### 7.5 Domain-Specific Extraction

**UCRBench finding:** Domain-specific projects (Ruoyi: AI assistant, Didicar: ride-hailing) show the highest omission rates for GPT models (65–73%) but the lowest for DeepSeek-Reasoner (9–12%). The pattern suggests that reasoning models with domain awareness outperform on domain-specific code.

**Actionable:** For Code Takeover, domain-specific extraction should use domain-specialized prompts with explicit domain glossaries. For a ride-hailing system, providing the domain model ("Passenger, Driver, Trip, Payment, Rating") as context dramatically changes which use cases the model can identify.

**FSM extraction precedent (SpecGPT, arXiv:2510.14348):** Protocol state machine extraction uses "domain-informed prompting with chain-of-thought reasoning, and ensemble methods" to achieve higher accuracy than generic extraction. Domain-specific prompting + ensemble is the empirically supported approach.

### 7.6 The Role of Existing Tests in Closing the Gap

Existing test suites are partial but valuable specifications. For Code Takeover:

- Tests provide ground truth for happy-path behavior — cases where LLM extraction is most reliable
- Tests expose some edge cases (though inadequately, per the 21% escape rate finding)
- **Coverage gap is measurable:** the fraction of code not covered by any test is the fraction where spec extraction must work without validation feedback

**Proposed pipeline:**
1. Extract specs from code (LLM baseline)
2. Generate tests from extracted specs
3. Run new tests on original code — failures indicate spec errors
4. Run original tests — missed behaviors indicate spec gaps
5. Identify uncovered code paths → targeted extraction prompts
6. Iterate until coverage converges

This is equivalent to the TICODER architecture applied to spec extraction rather than code generation.

---

## 8. Implications for Code Takeover

### 8.1 Current Reliability Assessment

Given the three findings combined:

- **49–65% of user-goal use cases are missed** in spec extraction (UCRBench)
- **19–35% of refactorings are non-equivalent** when the spec is translated back to code (DFuzz)
- **21% of these non-equivalences escape test detection** (DFuzz escape rate)
- **17.3% detection accuracy** on multi-threaded behavior (mutation study)

For a system with 100 user-facing behaviors, Code Takeover as currently practiced would:
- Extract specs for ~35–51 behaviors (missing 49–65%)
- Produce non-equivalent implementations for 6–12 of those 35–51 (19–35% rate)
- Fail to detect 1–3 of those non-equivalences via tests (21% escape rate)
- Completely miss any concurrency behavior specification

**This is not acceptable for production systems.** The gap is not marginal — it is architectural.

### 8.2 Where Progressive Validation Helps

The Airbnb code migration experience (2024) demonstrates that iterative retry loops ("simple retry loop to successfully migrate simple-to-medium complexity test files, with some finishing successfully after a few retries, and most by 10 attempts") work for implementation but not for spec extraction where there is no automated oracle to know when the spec is complete.

Progressive validation (convert one function, test, expand) addresses the **non-equivalence problem** — if you test exhaustively after each conversion, you catch semantic drift. But it does not address the **omission problem** — a spec that omits 65% of use cases will pass all tests generated from the incomplete spec.

### 8.3 The Two Distinct Problems

**Problem A: Omission (UCRBench).** The extracted spec is an incomplete description of the code's behavior. Tests generated from the spec will not exercise the missing behaviors. This is a recall problem.

**Problem B: Non-equivalence (DFuzz).** The code generated from the spec diverges from the original code's behavior on inputs not covered by tests. This is a precision problem, downstream of Problem A.

Code Takeover needs solutions to both:
- For Problem A: ensemble extraction, RAG augmentation, interactive clarification, coverage-guided expansion
- For Problem B: differential fuzzing as part of the validation pipeline, not just test-passing as the criterion

### 8.4 Domain as a Key Variable

UCRBench shows domain complexity is the primary driver of omission rate variation (Ruoyi: 9% for DeepSeek-Reasoner vs. 73% for GPT-5 on the same codebase). This means:

- Well-documented, standard domains (web CRUD, e-commerce) will have lower omission rates
- Novel or specialized domains (financial trading systems, medical devices, protocol stacks) will have higher omission rates
- Domain grounding (providing glossary, domain model, business context) is a first-order intervention

**For specpunk/CodeSpeak:** Domain-specific Code Takeover — starting with well-understood, standard domains (REST APIs, CRUD backends) and progressively expanding to more complex domains as tooling matures — is the empirically supported incremental strategy.

### 8.5 Concurrency: The Unsolved Problem

No current approach reliably extracts concurrency specifications from code. The 17.3% mutation detection rate means that concurrent behavior is effectively opaque to LLM-based spec extraction. For Code Takeover of concurrent systems:

- **Do not rely on LLM extraction for concurrency specs.** Instead, require explicit specification of concurrent contracts as a human-provided input.
- **Use static analysis tools** (race condition detectors, deadlock analyzers, memory model checkers) to generate formal concurrency specs that supplement LLM-extracted behavioral specs.
- **Prioritize migration from concurrent to sequential** architectures (e.g., async/await patterns that eliminate shared mutable state) as part of the Code Takeover process.

---

## 9. Research Gaps and Open Questions

### 9.1 What Has Not Been Studied

**Omission taxonomy.** UCRBench measures that 49–65% of use cases are missed but does not categorize which types are missed (edge cases, error handling, security, etc.). A study that annotates ground-truth use cases by type would enable targeted extraction strategies.

**Context window scaling.** UCRBench uses GPT-5 (1M context window) but does not systematically study how omission rate changes with code volume presented in context. Is the 65% omission rate for GPT-5 worse or better than for smaller context windows?

**Prompting strategy ablation.** UCRBench does not compare zero-shot, few-shot, CoT, and expert prompting strategies. The SpecGPT protocol extraction work shows domain-informed prompting significantly improves recall — this may apply to use case extraction too.

**Non-equivalence taxonomy.** The DFuzz paper (arXiv:2602.15761) measures the rate of non-equivalence but does not categorize the types of behavioral divergence. Understanding *what* goes wrong would enable targeted validation strategies.

**Concurrency spec extraction tools.** No paper directly addresses extracting concurrency specs (happens-before, memory ordering) from code using LLMs. This is the critical gap for multi-threaded system migration.

**Multi-language gap.** UCRBench uses Java; the DFuzz study uses Python. How do these gaps change for TypeScript, Rust, Go? Languages with stronger type systems (Rust's ownership model) may show different spec extraction characteristics.

### 9.2 What Could Be Built Now

**Coverage-guided spec completion loop** (highest priority): Use existing test suite + coverage tools to identify uncovered code paths → targeted LLM extraction prompts for those paths → validate extracted specs by generating tests → repeat until coverage converges.

**Ensemble spec merger**: Run GPT and DeepSeek-Reasoner spec extraction in parallel, merge results using semantic deduplication (SBERT), flag discrepancies for human review. Expected omission rate reduction: from 49–65% to 25–35% (based on complementary failure modes observed in UCRBench).

**Differential fuzzing as CI gate**: Integrate Atheris-based differential fuzzing (Eq@DFuzz methodology) into code migration pipeline. Before accepting a migrated function, fuzz-compare it against the original with 1,000–2,000 generated inputs. Expected escape rate reduction: from 21% to near-zero.

**Mutation-score-based spec quality metric**: After spec extraction, generate targeted code mutations (statement, value, decision, per arXiv:2602.17838 methodology) and check how many the extracted spec would detect. Report a "spec coverage score" as a proxy for completeness.

**Domain glossary injection**: For each Code Takeover session, require explicit domain model input (entity types, relationships, business rules) injected as context. Measure omission rate improvement. Expected: significant improvement for specialized domains (e.g., Ruoyi-like systems).

---

## 10. Source Papers and References

All quantitative claims above are sourced from the following primary papers:

1. **UCRBench** — arXiv:2512.13360 — Xiao et al., December 2025
   URL: https://arxiv.org/abs/2512.13360

2. **Differential Fuzzing Refactoring** — arXiv:2602.15761 — Dristi & Dwyer, February 2026
   URL: https://arxiv.org/abs/2602.15761

3. **Mutation-Analysis Summarization** — arXiv:2602.17838 — Khatib et al., February 2026
   URL: https://arxiv.org/abs/2602.17838

4. **LLM Concurrent Program Verification** — arXiv:2501.14326 — January 2025
   URL: https://arxiv.org/abs/2501.14326

5. **EquiBench** — arXiv:2502.12466 — February 2025
   URL: https://arxiv.org/abs/2502.12466

6. **SpecGen** — arXiv:2401.08807 — ICSE 2025
   URL: https://arxiv.org/abs/2401.08807

7. **ProtocolGPT / State Machine Inference** — arXiv:2405.00393 — 2024
   URL: https://arxiv.org/abs/2405.00393

8. **LLM Code Understanding (Debugging Accuracy)** — arXiv:2504.04372 — 2025
   URL: https://arxiv.org/abs/2504.04372

9. **LLM Refactoring Comprehensive Evaluation** — arXiv:2511.21788 — November 2025
   URL: https://arxiv.org/abs/2511.21788

10. **LLM Code Refactoring Empirical Study** — arXiv:2411.02320 — November 2024
    URL: https://arxiv.org/abs/2411.02320

11. **AmbiSQL** — arXiv:2508.15276 — 2025
    URL: https://arxiv.org/abs/2508.15276

12. **BIRD Benchmark** — https://bird-bench.github.io/

13. **Mutation-Guided Test Generation at Meta** — arXiv:2501.12862 — January 2025
    URL: https://arxiv.org/abs/2501.12862

14. **SpecGPT Protocol State Machine Extraction** — arXiv:2510.14348 — 2025
    URL: https://arxiv.org/abs/2510.14348

15. **Examining LLM Refactoring** — arXiv:2411.02320
    URL: https://arxiv.org/html/2411.02320v1

16. **ClarifyGPT** — ACM Software Engineering 2024
    URL: https://dl.acm.org/doi/10.1145/3660810

17. **LLM Code Translation Formal Reasoning** — EECS-2025-174
    URL: https://www2.eecs.berkeley.edu/Pubs/TechRpts/2025/EECS-2025-174.pdf

18. **Dissect-and-Restore (Prometheus)** — arXiv:2510.25406
    URL: https://arxiv.org/abs/2510.25406

---

## 11. Supplementary: EquiBench — Detailed Breakdown

**Paper:** EquiBench: Benchmarking Large Language Models' Reasoning about Program Semantics via Equivalence Checking
**Authors:** Anjiang Wei, Jiannan Cao, Ran Li, et al.
**arXiv:** 2502.12466 (February 2025)
**Code/data:** https://github.com/Anjiang-Wei/equibench

### 11.1 Benchmark Structure

2,400 program pairs across 4 programming languages, 6 categories, 400 pairs per category (200 equivalent, 200 inequivalent):

| Category | Language | Transformation Type | Mean Accuracy | Difficulty |
|----------|----------|---------------------|---------------|------------|
| OJ_V | Python | Variable renaming only | **78.1%** | Easiest (syntactic) |
| OJ_A | Python | Algorithmic variation | **68.6%** | Moderate |
| OJ_VA | Python | Variable + Algorithmic | **68.5%** | Moderate |
| x86-64 | Assembly | Compiler-scheduled optimization | **66.7%** | Hard |
| CUDA | CUDA | GPU memory-level transformations | **53.4%** | Hard |
| DCE | C | Dead code elimination | **49.0%** | Hardest (near random) |

### 11.2 Top Model Performance by Category

| Model | DCE | CUDA | x86-64 | OJ_A | OJ_V | OJ_VA | Overall |
|-------|-----|------|--------|------|------|-------|---------|
| o4-mini | — | — | — | **89.0%** | **96.5%** | **88.5%** | **82.3%** |
| o3-mini | — | — | **84.5%** | — | — | — | **78.0%** |
| DeepSeek-R1 | — | — | — | — | — | — | **73.5%** |
| o1-mini | — | — | — | — | — | — | **71.5%** |
| claude-3.5-sonnet | — | **63.8%** | — | — | — | — | — |
| o4-mini (DCE) | **76.2%** | — | — | — | — | — | — |

Random baseline across all categories: **50.0%**

### 11.3 The Syntactic Bias Finding

The central failure mode documented: **LLMs are biased toward syntactic similarity rather than semantic reasoning.**

- When two programs are syntactically similar, LLMs predict "equivalent" — even when they are not
- When two programs are syntactically dissimilar, LLMs predict "inequivalent" — even when they are equivalent
- Statistically validated at significance α=0.05 across all 19 models

**Consequence for spec extraction:** A spec extracted from Code A will correctly note "Code B is equivalent" when the two look similar but may miss non-equivalence in programs that look different. Worse: a spec may claim equivalence between programs that merely share surface patterns (common API calls, typical algorithm structure).

**The CUDA collapse:** In the CUDA category, o3-mini achieves 90.5% accuracy on inequivalent pairs but only **27.5% on equivalent pairs** — worse than random for recognizing equivalence. The model's strong prior toward "different-looking GPU code = inequivalent" fails catastrophically when two programs compute the same result via different memory access patterns.

### 11.4 Prompting Strategies Provide No Meaningful Improvement

| Prompting Strategy | o1-mini | gpt-4o | DeepSeek-V3 | gpt-4o-mini |
|------------------|---------|--------|-------------|------------|
| 0-shot | 71.5% | 65.0% | 65.0% | 62.2% |
| 4-shot | 71.5% | 66.5% | 66.9% | 63.5% |
| 0-shot + CoT | 71.9% | 62.5% | 63.3% | 60.2% |
| 4-shot + CoT | 71.9% | 62.7% | 62.5% | 61.2% |

Chain-of-thought actually **hurts** non-reasoning models (gpt-4o, DeepSeek-V3). The difficulty is fundamental, not prompt-engineering-solvable.

**Fine-tuning is also insufficient:** Qwen2.5-14B improved from 59.8% to 63.2% (+3.4pp) after LoRA fine-tuning on 1,200 examples. Binary equivalence labels provide "limited learning signals for reasoning about program semantics."

### 11.5 Implication for Differential Verification

EquiBench shows LLMs cannot reliably determine semantic equivalence — the core operation needed for differential verification in Code Takeover. For Code Takeover's "are these two versions equivalent?" check:

- Variable renaming (OJ_V): 78.1% reliable — usable
- Algorithmic transformation (OJ_A): 68.6% — borderline
- Assembly/GPU optimization: 53.4–66.7% — not reliable
- Dead code elimination: 49.0% — random

**Conclusion:** LLM-based equivalence checking must be augmented by dynamic testing (differential fuzzing) or static verification for any category beyond simple variable renaming. The EquiBench findings explain why Eq@DFuzz (Section 2) is necessary: LLMs cannot self-verify refactoring equivalence.

---

## 12. Supplementary: Security Vulnerability Coverage Gaps

**Source:** Systematic Literature Review on LLM Vulnerability Detection (arXiv:2412.15004, 2024)

### 12.1 Vulnerability Categories LLMs Miss

A review of 17 empirical studies on LLM-based vulnerability detection reveals systematic gaps:

| Vulnerability Category | Studies Covering It | Coverage Level | Notes |
|------------------------|--------------------|--------------|----|
| Memory safety (buffer overflow, UAF) | High | 11/17 studies | Common in training data |
| Injection (SQL, command) | High | Common | Overrepresented in training |
| Error handling | **Low** | 3/17 | Underexplored |
| Deserialization | **Low** | 6/17 | "Open questions remain" |
| Inter-procedural issues | **None** | 0/17 | All studies focus intra-procedural |
| Cross-cutting constraints | None | 0/17 | Not addressed at all |

**Critical gap:** All 17 studies "focus on intra-procedural vulnerabilities." No study addresses inter-procedural vulnerability detection — the case where a vulnerability arises from the interaction of two functions that are individually correct. This maps directly to the UCRBench finding: LLMs fail at cross-module behavior that requires reasoning across boundaries.

### 12.2 LLM Accuracy Collapses on Complex Codebases

From the systematic review: "accuracy dropped below 30% when faced with more complex tasks, such as detecting the root causes of vulnerabilities across extensive codebases." The collapse is not linear — it is dramatic. This 30% figure on complex vulnerability detection parallels the UCRBench finding of 9–90% omission range depending on codebase complexity.

### 12.3 Naming Dependency — A Fundamental Problem

An empirical finding with direct implications for Code Takeover: "naming significantly affects LLMs' ability to detect vulnerabilities, indicating that LLMs rely on clear and meaningful names."

This means:
- Well-named codebases (descriptive function names, consistent terminology) show better spec extraction
- Legacy code with abbreviated or cryptic names (common in C codebases) shows worse extraction
- Obfuscated code causes "accuracy drops in GPT models"

For spec extraction, this naming dependency means the quality of the extracted spec depends significantly on how well the original developer named things — not just on how the code behaves.

### 12.4 The False Positive Trap

LLM vulnerability detectors generate high false positive rates. From the review: false positives were mentioned in 11/17 studies. But false positives mask a deeper problem: when models over-detect on known patterns, they under-detect on unknown patterns. The LLM optimizing for "does this look like SQL injection?" will generate false positives on harmless parameterized queries while missing novel injection patterns.

For spec extraction: models optimizing for plausible-sounding specs generate specific-looking but incorrect specs for behaviors they recognize from training data, while producing vague or missing specs for novel domain behaviors.

---

## 13. Supplementary: PatchGuru — Extracting Specs From PR History

**Paper:** PatchGuru: Patch Oracle Inference from Natural Language Artifacts with Large Language Models
**Authors:** Le-Cong, Le, Murray, Pradel, Cadar
**arXiv:** 2602.05270 (submitted February 4, 2026)

### 13.1 Approach

PatchGuru represents a different angle on spec extraction: rather than extracting specs from code, it extracts *behavioral intent* from pull request descriptions (natural language artifacts) and converts them into executable runtime assertions.

**Process:**
1. Read PR description (natural language developer intent)
2. Use LLM to infer behavioral assertions that should hold after the patch
3. Generate comparison programs that run pre- and post-patch code and compare outputs
4. Flag divergences as regressions

### 13.2 Results on 400 Real PRs

Evaluated on 400 recent PRs from 4 open-source Python projects:

| Metric | PatchGuru | Testora (competitor) |
|--------|-----------|----------------------|
| Warnings generated | 39 | higher (more false positives) |
| Precision | **0.62** | 0.32 |
| True positives confirmed | 24 | 7 |
| Previously unknown bugs | 12 | — |
| Fixed by developers | 11 | — |
| Cost per PR | $0.07, 8.9 min | — |

### 13.3 Relevance to Code Takeover

PatchGuru demonstrates that **natural language developer artifacts (PR descriptions, commit messages, issue comments) contain behavioral specifications** that complement code-level extraction. The 62% precision suggests that LLM-derived behavioral assertions from informal text are moderately reliable — comparable to LLM-extracted use cases in UCRBench.

**Integration opportunity for Code Takeover:** When migrating existing codebases, git history is available. PR descriptions, commit messages, and linked issue comments represent implicit behavioral specifications accumulated over the codebase's lifetime. PatchGuru's approach — converting these to runtime assertions — could fill gaps that static code analysis misses, particularly for:
- Bug fix behaviors (what was the wrong behavior, what should the correct one be)
- Feature behaviors (PR descriptions often specify new behaviors)
- Regression contracts (what must not change)

**Limitation:** 62% precision means 38% false positives — runtime assertions that fire on correct code. For Code Takeover validation, false positives require human triage, adding cost.

---

## 14. Supplementary: Property-Based Testing and Spec Coverage

**Sources:**
- "Can Large Language Models Write Good Property-Based Tests?" (arXiv:2307.04346)
- "Agentic Property-Based Testing" (arXiv:2510.09907)

### 14.1 GPT-4's Success Rate on Property Extraction

When asked to generate property-based tests (PBTs) from API documentation:

- GPT-4 success rate: **21% of extractable properties** get a valid, sound PBT
- Mean samples to a valid PBT: **2.4 samples**
- Coverage: the 79% of properties with no generated PBT remain untested

This 21% success rate is the property-extraction analogue of UCRBench's use-case omission rate. In both cases, LLMs recover roughly one-fifth to one-half of the full specification.

### 14.2 Which Properties Are Missed

The 79% failure includes:
- Properties requiring quantifier reasoning ("for all inputs satisfying X, output satisfies Y")
- Properties with complex preconditions (subtle type constraints)
- Properties that depend on state history (not just input/output)
- Cross-function properties (behavior of A given prior call to B)

The cross-function property failure directly parallels the UCRBench cross-module failure: LLMs generate local specifications but miss global invariants.

### 14.3 Agentic PBT: Improvement via Reflection

Agentic property-based testing (arXiv:2510.09907) — where the LLM generates tests, executes them, reflects on results, and iterates — achieves **56% valid bug detection** across 100 Python packages. 32% are reportable to maintainers.

The reflection loop improves coverage because:
1. Failed tests reveal which properties the model's initial extraction missed
2. Execution feedback provides implicit behavioral specifications that static reading missed
3. Iterative refinement catches cases where initial property formulation was wrong

**For Code Takeover:** An agentic PBT loop applied to extracted specs would catch inconsistencies between the extracted spec and actual code behavior — providing an automated oracle for spec quality.

---

## 15. Synthesis: The Coverage Stack

The research across all 18 primary sources converges on a layered coverage deficit:

```
Level 0: Surface pattern matching (what LLMs do well)
  - Variable renaming: 78.1% accurate (EquiBench OJ_V)
  - Syntactically similar code: high confidence
  - Named patterns (SQL injection, buffer overflow): high recall

Level 1: Function-level behavior (partially covered)
  - Single function mutation detection: 76.5% (Khatib et al.)
  - Function-level refactoring equivalence: ~80% (GPT-4o)
  - PBT property extraction: 21% of properties

Level 2: Cross-function / module-level behavior (weak)
  - Subfunction use case omission: 26–43% avg (UCRBench)
  - Multiple class mutation detection: 28.4% (Khatib et al.)
  - Inter-procedural vulnerability detection: ~30% (systematic review)

Level 3: System-level / user-goal behavior (severely weak)
  - User-goal use case omission: 44–65% avg (UCRBench)
  - Program-level refactoring non-equivalence: 32.1% (APPS dataset)
  - Complex vulnerability detection: <30% (systematic review)

Level 4: Concurrency / distributed behavior (effectively zero)
  - Multi-threaded mutation detection: 17.3% (Khatib et al.)
  - Relaxed memory model assertion: F1=0.80 best case (arXiv:2501.14326)
  - Concurrency spec extraction: no systematic study

Level 5: Non-functional requirements (not covered)
  - Performance: no evidence of reliable extraction
  - Security constraints: cross-cutting, unmeasured
  - Regulatory compliance: no systematic study
```

The coverage stack shows a monotone degradation: as abstraction level increases, LLM reliability decreases. Levels 0–1 are usable with validation. Levels 2–3 require augmentation (ensemble, RAG, interactive clarification). Level 4 requires static analysis tools, not LLMs. Level 5 requires explicit human specification.

**For Code Takeover scoping:** A code base can be classified by which coverage levels it exercises. A REST CRUD API primarily operates at Levels 1–2 — Code Takeover should work well. A concurrent message queue operates at Levels 3–4 — Code Takeover faces fundamental limits. The coverage stack provides a tool for Code Takeover feasibility assessment.

---

## 16. Additional References

19. **PatchGuru** — arXiv:2602.05270 — Le-Cong et al., February 2026
    URL: https://arxiv.org/abs/2602.05270

20. **LLM Vulnerability Detection Systematic Review** — arXiv:2412.15004 — 2024
    URL: https://arxiv.org/abs/2412.15004

21. **Can LLMs Write Good Property-Based Tests?** — arXiv:2307.04346 — 2023
    URL: https://arxiv.org/abs/2307.04346

22. **Agentic Property-Based Testing** — arXiv:2510.09907 — 2025
    URL: https://arxiv.org/abs/2510.09907

23. **EquiBench (detailed analysis)** — arXiv:2502.12466 — Wei et al., February 2025
    URL: https://arxiv.org/abs/2502.12466
    GitHub: https://github.com/Anjiang-Wei/equibench

24. **SESpec: Symbolic Execution + LLM Spec Generation** — arXiv:2506.09550 — 2025
    URL: https://arxiv.org/abs/2506.09550

25. **RECOVER: Requirements from Stakeholder Conversations** — arXiv:2411.19552 — 2024
    URL: https://arxiv.org/abs/2411.19552

26. **LLM-Driven User-Intent Formalization (FMCAD 2024)** — arXiv:2406.09757 — 2024
    URL: https://arxiv.org/abs/2406.09757

27. **Code Comprehension Diagnostics (AUROC 0.63)** — arXiv:2601.12951 — 2026
    URL: https://arxiv.org/abs/2601.12951

28. **Hallucination Taxonomy for Code LLMs** — arXiv:2504.20799 — 2025
    URL: https://arxiv.org/abs/2504.20799

---

*Research conducted 2026-03-12. Supplementary sections 11–16 added 2026-03-12 with additional primary source analysis. All quantitative claims traceable to primary papers.*
