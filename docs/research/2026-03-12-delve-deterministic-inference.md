# Deterministic LLM Inference — Breaking the Non-Determinism Barrier for Spec-Driven Development

**Date:** 2026-03-12
**Type:** Deep Research
**Status:** Complete
**Research method:** Web search + primary source fetch, 9 sub-questions

---

## Executive Summary

Non-determinism in LLM inference is not just a nuisance — it is a structural blocker for spec-driven development (SDD). When `codespeak build` runs twice from the same spec and produces different code, you cannot snapshot-test, reproducibly debug, cache builds, or treat specs as the true source artifact. The problem is multi-layered: sampling stochasticity is the visible symptom, but the deeper cause is floating-point non-associativity in GPU kernels whose reduction order shifts with batch size.

Two parallel research tracks emerged in late 2025:

1. **Batch-invariant kernels** (Thinking Machines Lab, SGLang team) — redesign reduction-heavy operations to produce identical outputs regardless of batch state. Cost: 34–62% latency overhead. Currently H100/H200-class only.
2. **Decode-verify-rollback** (LLM-42, Microsoft Research) — keep fast optimized kernels on the hot path, verify determinism post-hoc, roll back on mismatch. Cost: proportional to deterministic traffic fraction (~6% at 100% deterministic traffic, ~3% at 2% deterministic traffic).

For SDD tools, neither approach is available through public APIs today. Anthropic, OpenAI, and Google all offer "mostly deterministic" settings (temperature=0, seed parameter) that are explicitly not guaranteed. In practice, even greedy decoding with identical seeds produces different outputs across runs due to dynamic batching at the infrastructure level.

The practical near-term path is: exact-match content-addressable caching + semantic equivalence checking as fallback + model version pinning. Not ideal, but achievable today without waiting for inference infrastructure to change.

---

## 1. Thinking Machines Lab: What They Actually Did

### 1.1 Background

Thinking Machines Lab was founded in February 2025 by Mira Murati (ex-CTO, OpenAI). Their first public research, released in November 2025, directly targeted inference non-determinism — a problem Murati observed as a practical blocker for reliable AI systems at OpenAI scale.

- **Blog post:** https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference/
- **Code:** https://github.com/thinking-machines-lab/batch_invariant_ops
- **Simon Willison commentary:** https://simonwillison.net/2025/Sep/11/defeating-nondeterminism/
- **Community discussion:** https://community.openai.com/t/defeating-nondeterminism-in-llm-inference/1358623

### 1.2 The Root Cause They Identified

The conventional wisdom was that LLM non-determinism stems from concurrent GPU execution — race conditions between threads. Thinking Machines Lab challenged this framing. Their finding:

> "The primary reason nearly all LLM inference endpoints are nondeterministic is that the load (and thus batch-size) nondeterministically varies."

When batch size changes, GPU kernels change their internal reduction strategies. Different reduction strategies mean different summation orders. Because floating-point arithmetic is non-associative — `(a+b)+c ≠ a+(b+c)` in finite precision — different summation orders produce different intermediate values, which cascade through the remaining computation.

A concrete example from the paper: `torch.mm(a[:1], b)` and `torch.mm(a, b)[:1]` — extracting one row before vs. after a matrix multiply — differ by **1669.25** due to different Split-K kernel selection. This is not a rounding error; it is a fundamentally different computation path.

### 1.3 The Three Operations That Break Determinism

**RMSNorm**

Root Mean Square Normalization reduces across batch elements. With different batch sizes, the GPU assigns different numbers of cores, producing different parallel reduction trees. Solution: assign one batch element to one core (data-parallel strategy) regardless of batch size. Small batches sacrifice parallelism but gain determinism.

**Matrix Multiplication**

Split-K optimization decomposes the K dimension across multiple GPU SMs to improve parallelism. The number of splits is batch-size-dependent. With different splits, the partial sum reduction order changes. Solution: fix 2D tile sizes across all batch sizes; avoid Split-K. Performance cost is ~20% vs. cuBLAS.

**Attention**

FlashAttention uses Split-KV strategies where the number of splits depends on how many tokens are processed. Different token counts → different split counts → different reduction order. Solution: use a **fixed split-size** (not a fixed split-count). When split size is fixed, the reduction order is invariant to how many total tokens are in the batch. Additionally: the KV cache must be updated before the attention kernel runs to ensure consistent cache layout regardless of batch composition.

### 1.4 Implementation Details

The library (`batch_invariant_ops`) uses `torch.Library` to substitute existing PyTorch kernels without requiring model code changes. The substituted operations:
- `torch.mm()` — batch-invariant matrix multiplication
- `torch.addmm()` — matmul with bias
- `torch.log_softmax()` — log-softmax for logit computation
- `torch.mean()` — reduction for RMSNorm

This is non-invasive: existing models written against standard PyTorch operators automatically get the batch-invariant versions when the library is loaded.

### 1.5 Benchmark Results

Test setup: 1,000 sequences with Qwen/Qwen3-235B-A22B-Instruct-2507, 90–110 token outputs, temperature=0, single GPU.

**Without batch-invariant kernels:** 80 unique completions out of 1,000 runs. All 1,000 shared the first 102 tokens, then diverged around token 103. Token 103 is where the logit gap between competing tokens was smallest — numerical noise was sufficient to flip the selection.

**With batch-invariant kernels:** 1,000 out of 1,000 completions identical.

| Configuration | Latency (1,000 sequences) |
|---|---|
| vLLM default | 26 seconds |
| Unoptimized deterministic (early version) | 55 seconds |
| + improved attention kernel | 42 seconds |

**Overhead: 62%** (26s → 42s). The lab notes this overhead is primarily due to early-stage kernel design, not a theoretical lower bound.

### 1.6 Secondary Benefit: On-Policy RL

A significant motivation beyond reproducibility: deterministic inference eliminates the KL divergence between sampling policy (used during RLHF rollouts) and training policy (used for gradient computation). Current inference non-determinism means RL is inadvertently off-policy. With batch-invariant kernels, on-policy training achieves true zero KL divergence.

---

## 2. Sources of LLM Non-Determinism: Complete Taxonomy

Eight distinct sources have been identified across the literature. Not all are equally significant in practice.

### 2a. Sampling Stochasticity (Tier 1: Intended)

Temperature scaling converts logits to a probability distribution; top-p (nucleus sampling) restricts sampling to the smallest token set covering probability mass p. At temperature > 0, the model randomly samples from this distribution. This is **intentional non-determinism** — it is the feature, not a bug. Setting temperature=0 eliminates this tier (greedy selection of the argmax token).

Impact on SDD: Full control is possible. Use temperature=0 for compilation tasks.

### 2b. Floating-Point Non-Associativity (Tier 2: Critical)

The root of all downstream non-determinism. In IEEE 754 arithmetic:
- FP32: 23 mantissa bits → rounding error ≈ 10⁻⁷
- FP16: 10 mantissa bits → rounding error ≈ 10⁻³
- BF16: 7 mantissa bits → rounding error ≈ 10⁻²

BF16 is the standard training and inference precision for modern LLMs (smaller footprint, better range than FP16). Its limited mantissa means accumulated rounding errors are much larger than FP32. When token logits for competing candidates differ by less than the accumulated rounding error, the winner flips based on computation order.

Source: https://arxiv.org/abs/2506.09501 (NeurIPS 2025)

Impact: Cannot be eliminated without changing precision or redesigning kernels.

### 2c. Dynamic Batching / Continuous Batching (Tier 2: Critical)

Modern inference servers (vLLM, SGLang, TGI) use continuous batching: new requests join in-flight batches dynamically. Each time the batch composition changes, kernel dispatch decisions change (tile sizes, split strategies, parallelism degrees). This is the primary mechanism demonstrated in the Thinking Machines Lab paper — non-determinism even at temperature=0.

The cascade: Same request, different batch context → different kernel path → different numerical output → different token selected → diverging sequences from token 103 onward.

Impact: The most important practical source. Controllable only by running single-request batches (unacceptable performance cost) or using batch-invariant kernels.

### 2d. Tensor Parallelism and AllReduce (Tier 2: Significant)

For models too large to fit on one GPU, tensor parallelism splits weight matrices across multiple GPUs. After each parallel computation, an AllReduce operation aggregates partial results via NCCL. Since AllReduce order is not guaranteed (multiple concurrent reduction trees), the accumulated result differs.

Quantified: Deterministic outputs occur for identical `tensor_parallel_size` but diverge across different TP configurations. AllReduce contributes up to 30% of end-to-end latency and is itself a non-determinism source independent of batch effects.

Impact: Affects any model requiring multi-GPU deployment (70B+ class). Cannot be easily fixed without synchronized reduction ordering.

### 2e. GPU Architecture and Count Effects (Tier 2: Significant, Often Ignored)

The NeurIPS 2025 paper (https://arxiv.org/abs/2506.09501) ran a systematic study across 12 configurations: L40S vs A100, 2 GPU vs 4 GPU, batch size 8/16/32.

Key finding: **DeepSeek-R1-Distill-Qwen-7B on AIME'24 under BF16 showed up to 9% accuracy variance and 9,000 token output length variance across hardware configurations with identical seeds.** The same model, same prompts, same temperature=0, different GPUs → 9% accuracy swing.

| Precision | Std@Acc (AIME'24) | Max output length variance |
|---|---|---|
| BF16 | 9.15% | 9,189 tokens |
| FP16 | 5.74% | — |
| FP32 | 0% | 0 tokens |

This is not a model quality issue. It is a precision + kernel scheduling issue that affects cloud deployments universally.

### 2f. Speculative Decoding Draft Model Divergence (Tier 3: Moderate)

Speculative decoding uses a small draft model to propose multiple tokens; a larger verifier model checks them in parallel. The draft model's token acceptance depends on KL divergence between draft and target distributions. Different acceptance patterns with different draft model states → different final output sequences, even when the target model's verification is deterministic.

Impact: If the inference stack uses speculative decoding for latency optimization, it adds another non-determinism source.

### 2g. KV Cache Quantization (Tier 3: Moderate)

KV cache quantization (INT8, FP8, NVFP4) compresses stored attention key-value pairs to save memory. The quantization error accumulates across layers and generation steps. Research shows these errors are "particularly damaging for code generation benchmarks where small numerical errors can cause syntax or logic failures."

LayerCast (from the NeurIPS 2025 paper) — performing all computations in FP32 while storing weights in BF16 — reduces divergence rates below 3.4% while cutting memory 34% vs full FP32 inference. This is the most practical near-term precision fix available without kernel redesign.

### 2h. Context Window Position Effects (Tier 3: Low)

For very long contexts, attention score computation across distant positions accumulates more floating-point error than short contexts. Minor secondary effect, but compounds with other sources at very long context lengths (>32K tokens).

### Priority Ranking for SDD Practitioners

| Source | SDD Impact | Controllable Today |
|---|---|---|
| Dynamic batching | Critical | No (needs batch-invariant kernels) |
| Float non-associativity (BF16) | Critical | Partially (use FP32: +memory, no API support) |
| Tensor parallelism / AllReduce | Significant | No |
| GPU architecture variation | Significant | No (cloud is heterogeneous) |
| Sampling temperature | Full control | Yes (set temperature=0) |
| Speculative decoding | Moderate | Yes (disable) |
| KV cache quantization | Moderate | Yes (disable quantization) |
| Context position effects | Low | No |

---

## 3. API-Level Determinism: What Providers Actually Guarantee

### 3.1 OpenAI

**Settings:** `seed` parameter + `temperature=0`

**What they say:** "We make a best effort to sample deterministically, such that repeated requests with the same seed and parameters should return the same result."

**What they actually guarantee:** Nothing. The `system_fingerprint` field indicates backend version; even when fingerprints match, "there is a small chance that responses differ."

**Evidence of failure:** "Even in cases where the seed parameter and system_fingerprint are the same across API calls it's currently not uncommon to still observe a degree of variability in responses." Additionally, larger `max_tokens` values produce less deterministic responses even with seed set.

Source: https://cookbook.openai.com/examples/reproducible_outputs_with_the_seed_parameter

### 3.2 Anthropic

**Settings:** `temperature=0`

**What they say (docs):** "Even with temperature 0.0, the results will not be fully deterministic."

**What they actually guarantee:** Nothing.

**Evidence of failure (GitHub issue, July 2025):** Claude CLI (`claude -p`) produces different outputs for identical inputs across calls, confirmed for Claude 4 models. Reporter notes: "Gemini 2.5 Flash provides deterministic output while Claude CLI does not."

Source: https://github.com/anthropics/claude-code/issues/3370

**Key difference from OpenAI:** Anthropic exposes no `seed` parameter in the public API. OpenAI's `seed` at least makes the goal explicit; Anthropic doesn't surface it.

**Extended thinking:** Claude's extended thinking mode (Claude 3.7 Sonnet+) adds another non-determinism layer. The thinking budget determines how many reasoning steps are taken, but the content of those steps is stochastic. Two identical prompts with thinking enabled can take different reasoning paths to the same or different conclusions.

### 3.3 Google (Gemini)

**Settings:** `temperature=0`, `seed` parameter (Vertex AI)

**What they say:** "A temperature of 0 means that the highest probability tokens are always selected. In this case, responses for a given prompt are mostly deterministic, but a small amount of variation is still possible."

**Evidence of failure (2025 GitHub issue):** The `gemini-2.5-pro` model "is producing different outputs for identical requests, even when a fixed seed is provided along with a constant temperature. This behavior has been reliably reproduced and violates the API's core contract for deterministic generation."

Source: https://discuss.ai.google.dev/t/the-gemini-api-is-exhibiting-non-deterministic-behavior-for-the-gemini-2-5-pro-model/101331

### 3.4 Self-Hosted: vLLM

With temperature=0, a per-request `seed`, and multiprocessing disabled, vLLM V1 (default since 2025) approaches determinism for fixed batch sizes. Dynamic batching from concurrent requests still breaks this.

From vLLM documentation: set `temperature=0`, `top_p=1`, per-request `seed=42`, and disable multiprocessing for V1 engine to make scheduling deterministic. This achieves determinism only for single-request isolation.

### 3.5 Self-Hosted: SGLang ≥0.5.3

SGLang integrated batch-invariant kernels in September 2025, building on Thinking Machines Lab's work:
- FlashInfer: fixed split-KV via `fixed_split_size` parameter
- FlashAttention-3: num-splits fixed to 1
- Triton: deterministic operation on AMD hardware via alignment size configuration
- Chunked prefill: aligned to integer multiples of split_kv_size
- Custom `multinomial_with_seed`: Gumbel noise from seeded hash function (reproducible non-greedy sampling)
- CUDA graphs enabled: **2.79× speedup**, reducing overhead to **34.35%** vs normal mode

Consistency validation: 50 trials → single unique output in deterministic mode vs. 3-4 unique outputs in normal mode.

Source: https://lmsys.org/blog/2025-09-22-sglang-deterministic/

SGLang achieves substantially better overhead than the original TML work (34% vs 62%) by combining batch-invariant kernels with CUDA graph optimization. H100/H200 GPUs required; AMD supported via Triton backend.

### 3.6 Summary Table

| Provider | Best Settings | Guarantee | Hardware Req | Overhead |
|---|---|---|---|---|
| OpenAI API | seed + temp=0 | Best effort | N/A | N/A |
| Anthropic API | temp=0 | None stated | N/A | N/A |
| Google Vertex AI | seed + temp=0 | Best effort | N/A | N/A |
| vLLM (self-hosted) | temp=0 + seed + no-multiprocess | Partial (single request only) | Any GPU | ~0% if no concurrency |
| SGLang ≥0.5.3 | deterministic mode | Yes | H100/H200 or AMD | 34% |
| TML batch_invariant_ops | batch-invariant kernels | Yes | H100 (SM90+) | 62% |

---

## 4. Semantic Determinism vs. Bitwise Determinism

### 4.1 The Distinction

**Bitwise determinism:** Every bit of the output is identical across runs. This is what Thinking Machines Lab achieved.

**Semantic determinism:** Outputs are functionally equivalent — they compile, pass the same tests, implement the same behavior — but may differ in variable names, whitespace, method ordering.

**Behavioral determinism:** Weakest form — only externally observable behavior matches (same API contract, same outputs for all inputs). The minimum bar for SDD.

### 4.2 Which Does SDD Need?

| Use Case | Minimum Required | Why |
|---|---|---|
| CI/CD regression testing | Semantic | Tests are the oracle |
| Debugging reproducibility | Bitwise | Must reproduce exact failure |
| Code review (PR diffs) | Semantic | Textual diff must be meaningful |
| Team collaboration (your build ≠ my build) | Bitwise | Semantic checking across team is not practical |
| Content-addressable caching | Bitwise | Semantic equivalence as cache key is the hard research problem |

### 4.3 LLM Performance on Equivalence Checking

**EquiBench** (EMNLP 2025) benchmarks LLMs' ability to determine whether two programs are semantically equivalent. The benchmark covers Python, C, CUDA, and x86-64 assembly.

Results (19 LLMs evaluated):
- Best model (o4-mini): 82.3% overall accuracy
- OJ_V (variable renaming): 78.1% mean accuracy
- OJ_A (algorithmic differences): 68.6% mean accuracy
- CUDA (tensor scheduling): 53.4% mean accuracy — barely above random
- DCE (dead code elimination): 49.0% mean accuracy — worse than random

Key finding: Models "often rely on superficial form features such as syntactic similarity rather than demonstrating robust semantic reasoning." Even advanced prompting (few-shot, chain-of-thought) "barely improve performance."

Source: https://arxiv.org/abs/2502.12466

**Implication for SDD:** Using an LLM to check whether two code generations are semantically equivalent is unreliable for anything beyond trivial structural transformations. CUDA-level equivalence (relevant for GPU code generation) is near-random.

### 4.4 Formal Verification Approaches

**Clover** (Stanford, SAIV 2024) achieves closed-loop verifiable code generation through a six-check consistency system:

1. **anno-sound:** Dafny verifies code satisfies formal annotations
2. **anno-complete:** LLM regenerates code from annotations; functional equivalence checked
3. **anno2doc:** LLM derives docstring from annotation; semantic equivalence verified
4. **doc2anno:** LLM generates annotation from docstring; logical equivalence checked
5. **code2doc:** LLM produces docstring from code
6. **doc2code:** LLM synthesizes code from docstring; functional equivalence verified

Results on CloverBench (60 textbook-level Dafny programs):
- Single run: 75% acceptance rate for correct instances, **0% false positive rate**
- 10 independent runs: 87% acceptance rate

Source: https://arxiv.org/abs/2310.17807

**Limitation:** Requires programs expressible in Dafny. Not practical for general-purpose code generation.

**Symbolic execution:** Formally proves input-output equivalence for all possible inputs. Accurate but "computationally intractable for real-world programs" (scalability issues cited universally).

**Bounded model checking:** Proves equivalence for inputs up to a given bound. Practical for algorithmic code, incomplete for general cases.

**Operational semantics / test oracle:** If generated code A and generated code B both pass the same comprehensive test suite, they are "behaviorally equivalent" by operational definition. Cheap, incomplete, practical.

---

## 5. Caching and Memoization Approaches

### 5.1 Exact-Match Content-Addressable Caching

The simplest approach: hash the full prompt (spec text + system prompt + model ID + sampling parameters) → cache the response. On cache hit, return the stored response without inference.

```
cache_key = sha256(model_id || temperature || spec_text || system_prompt)
if key in cache: return cache[key]
response = model.generate(prompt)
cache[key] = response
return response
```

This is **bitwise deterministic** for repeated identical prompts from the same build context. It eliminates the non-determinism problem for CI/CD entirely — the first run is non-deterministic, all subsequent runs replay the cached result.

**Propel engineering blog:** "Content-addressable caching becomes practical: key caches by a hash of inputs (prompt, tool calls, retrieved context) and reuse results across requests, regions, and deployments."

Source: https://www.propelcode.ai/blog/defeating-nondeterminism-in-llm-inference-ramifications

**Key limitation:** Cache invalidation. Any change to the spec — even whitespace — produces a cache miss and a new (potentially different) output. Model version upgrades invalidate the entire cache.

### 5.2 Semantic Caching

Semantic caching stores responses based on meaning rather than exact text. If two prompts are semantically similar above a threshold, return the same cached response.

**Architecture:**
1. Embed the incoming prompt using a sentence transformer (typically 384-dim vector)
2. Search vector store for nearest cached prompt (cosine or Euclidean similarity)
3. If similarity > threshold: return cached response
4. Otherwise: invoke LLM, store (embedding, response) pair

**Tools:**
- GPTCache (Zilliz): https://github.com/zilliztech/GPTCache — open-source, integrated with LangChain and llama_index
- LMCache: https://arxiv.org/abs/2510.09665 — enterprise-grade distributed KV cache management

**Performance (2025 production benchmarks):**
- 31% of production LLM queries exhibit semantic similarity to previous queries
- Hybrid 3-tier caching engine: 87.5% cache hit rate on 100 real Anthropic API calls
- LLM-based equivalence detection: 67% cache hit rate
- 71.8% API cost reduction in empirical testing
- 2–10× speedup for cache hits

Source: https://arxiv.org/abs/2411.05276

**Systematic framework** (https://arxiv.org/abs/2508.07675) models semantic caching as an online optimization problem using Contextual Upper Confidence Bounds:
- Online adaptive algorithm: at least 11.75% improvement over static baselines
- Low-switching variant: 90.91% reduction in cache updates with comparable performance
- Regret bound: O(√(mT log(mT)) log log T) — sublinear in time

**For SDD specifically:** The mismatch cost in code generation is very high — returning wrong code for a spec is catastrophic, not just suboptimal. This means the similarity threshold must be extremely conservative, drastically reducing effective hit rates. Semantic caching is better suited for chatbot applications than for spec compilation.

### 5.3 Prefix Caching (Provider KV Caching)

Distinct from semantic caching: Anthropic and OpenAI cache the KV states for prompt prefixes that recur across API calls.

- Anthropic: Explicit `cache_control` breakpoints; 90% cost reduction, 85% latency reduction for long prompts
- OpenAI: Automatic for prompts ≥1024 tokens; 50% cost savings
- SGLang/vLLM: Radix tree for automatic prefix caching

**Important:** Prefix caching does not help with non-determinism. The cached KV states represent the model's computation of the prefix; the generation step downstream is still non-deterministic due to dynamic batching of in-flight requests.

---

## 6. Impact on Spec-Driven Development Workflows

### 6.1 Code Review (PR Diffs Become Noise)

When a developer updates a spec and regenerates code, the textual diff should reflect only the semantic change. With non-deterministic generation:

- Unchanged spec sections regenerate different code → noise in the diff
- Reviewers cannot distinguish intentional changes from generation variance
- Code review becomes unreliable as a semantic verification mechanism

Tessl's `// GENERATED FROM SPEC - DO NOT EDIT` pattern is designed to exclude generated code from human review. But this only works if reviewers trust the spec as the authoritative record. Non-determinism breaks that trust — the spec says one thing, the generated code is a stochastic sample from the spec's implied distribution.

**Workaround in practice:** Lock generated code in version control after the first generation. Only regenerate when the spec explicitly changes. Use the committed generated code as ground truth, not the spec-at-build-time.

### 6.2 Debugging (Bugs Become Unreproducible)

The NeurIPS 2025 study showed DeepSeek-R1 producing 9,000-token output length variance across hardware configurations. For a debugging session:

- Bug appears in generated code from a specific build
- Developer tries to reproduce by rebuilding from spec
- Regeneration produces different code (possibly without the bug)
- The bug is now unreproducible by construction

This is worse than traditional debugging. At least deterministic code can be reasoned about consistently. With non-deterministic generation, a bug may be a transient property of a particular generation run that will never appear again.

**Workaround:** Commit generated code to version control immediately after generation. Treat each generation as an immutable artifact. Store the cache key (hash of inputs) alongside the artifact.

### 6.3 CI/CD (Builds Not Reproducible)

For reproducible builds, the standard is: same source inputs → same binary output. With LLM code generation:

- Same spec + same model → different code on each run
- CI passes for developer A, fails for developer B with identical spec
- Build logs show different code being compiled across runs

**Empirical measurement** (ACM TOSEM, 2024):
- 75.76% of coding tasks produced zero identical test outputs across different requests (HumanEval dataset)
- Maximum test pass rate difference of 1.00 (100%) for 39.63% of HumanEval problems
- Temperature=0 reduces but does not eliminate this variance

Source: https://dl.acm.org/doi/10.1145/3697010

**Workaround:** Cache first build output, use it for all subsequent runs. Treat spec→code as a sealed compilation step. Regenerate only on explicit cache invalidation triggered by a spec change.

### 6.4 Team Collaboration (Your Build ≠ My Build)

With traditional source code: `git clone` + `build` → identical binary. With spec-as-source:
- Developer A generates from spec on their machine
- Developer B generates from same spec on their machine
- A's code ≠ B's code
- Both commit "their" version of the generated code → merge conflict on semantically identical but textually different artifacts

**Workaround (Tessl's observed pattern):** Commit generated code to VCS. The spec is the editorial artifact (humans edit specs), but the generated code is deterministic once committed — it is locked in git. This effectively collapses spec-as-source back to code-as-source from the team coordination perspective.

### 6.5 What SDD Tools Actually Do in 2026

**CodeSpeak:** Treats tests as the semantic oracle. Generates test vectors that verify against real files. Acknowledges that "when editing the spec, we can generate adequate changes in the code (spec diff → code diff)" is still future work. The spec change → adequate code change equivalence is not yet guaranteed.

Source: https://codespeak.dev/blog/codespeak-takeover-20260223

**Tessl:** Uses structured specs as persistent context. Spec Registry provides version-accurate library APIs (10,000+ specs) to reduce hallucination. Does not claim deterministic generation; generated code is committed to version control with `// GENERATED FROM SPEC - DO NOT EDIT` annotations.

Source: https://tessl.io/blog/how-tessls-products-pioneer-spec-driven-development/

**Martin Fowler analysis (March 2026):** Tessl's Böckeler "experienced non-determinism when generating code multiple times from identical specs" and describes iterating on spec precision to increase repeatability. Explicit conclusion: more precise specs reduce but do not eliminate non-determinism. She draws parallels to historical Model-Driven Development failures, questioning whether combining "inflexibility and non-determinism" might create unforeseen problems with spec-as-source approaches.

Source: https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html

---

## 7. Model Versioning and Long-Term Reproducibility

### 7.1 The Deprecation Problem

Even with perfect inference determinism (same model → same output), model updates break reproducibility. Standard provider behavior:

- **OpenAI:** Dated snapshots (`gpt-4o-2024-08-06`). Point aliases (`gpt-4o`) silently redirect to newer versions. Old snapshots deprecated with notice but eventually removed.
- **Anthropic:** Descriptive tiers (`claude-opus-4-5`). No public commitment to long-term snapshot retention.
- **Google:** Generation markers (`gemini-1.5-pro-002`). Vertex AI supports explicit version pinning.

**Reproducibility crisis in SE research:** An empirical study of 640 papers (2017–2025) at premier SE/ML venues found that missing reproducibility elements appear in 32.2% of papers, rising from 12.5% in 2022 to over 40% in 2024–2025. Most common issues: missing dependency specs, unpinned library versions, vague model references ("latest release").

Source: https://arxiv.org/abs/2512.00651

### 7.2 When a Model Is Deprecated: The Migration Problem

For spec-as-source, model deprecation is equivalent to a breaking compiler change. If model version X generated code from spec S, and model version Y now generates different code from spec S:

- All previously generated artifacts must be regenerated
- Regenerated code may not be semantically equivalent to the original
- Tests may need to be updated (or they catch regressions)
- The spec itself may need to be updated to produce semantically equivalent code from the new model

From SDD literature:

> "With AI-generated code, a code issue is an outcome of a gap in the specification, and because of non-determinism in AI generation, that gap keeps resurfacing in different forms whenever the code is regenerated."
>
> "Stable regeneration is hard — spec-driven development is an iterative approach, not a magic 'generate once' solution, and each regeneration might produce slightly different implementations."

### 7.3 Best Practices for Model Version Management

1. **Pin model versions explicitly.** Use dated snapshots, not point aliases. Store the model version in the build manifest alongside the spec hash.

2. **Store model commit hashes for open-weight models.** vLLM self-hosted deployments can pin to specific Hugging Face model commits. This is the only way to achieve true reproducibility for self-hosted inference.

3. **Treat model upgrades as breaking changes.** When upgrading the generation model, re-run all specs and perform semantic equivalence checking on all generated artifacts before committing.

4. **Use DVC (Data Version Control) for model artifacts.** DVC integrates with CI/CD pipelines to track model snapshots and enable rollback.

Source: https://circleci.com/blog/automated-version-control-for-llms-using-dvc-and-ci-cd/

5. **Record generation timestamp and system fingerprint.** OpenAI's `system_fingerprint` field indicates backend version. Store this alongside generated artifacts to detect silent backend changes.

**Build manifest pattern:**
```toml
[build]
model = "claude-opus-4-5"           # pinned, not "claude-opus"
model_fingerprint = "sha256:abc123" # optional
generated_at = "2026-03-12T14:30:00Z"
cache_key = "sha256:def456"         # hash of all inputs
spec_version = "1.2.0"
```

---

## 8. Alternative Architectures for More Deterministic Inference

### 8.1 State Space Models (Mamba)

Mamba (Gu & Dao, 2023) uses selective state space models instead of attention. During inference, Mamba runs recurrently — processing one token at a time — rather than attending to all previous tokens.

**Determinism properties:**
- Autoregressive SSM inference is a fixed recurrence: `h_t = A·h_{t-1} + B·x_t`, `y_t = C·h_t`. Each step is a deterministic matrix-vector product.
- No attention mechanism → no FlashAttention split-KV non-determinism
- No KV cache with associated quantization artifacts
- Constant memory footprint per step regardless of sequence length

**Performance advantages:**
- 5× higher inference throughput than transformers at equivalent parameter count
- O(1) time per step vs O(n) attention
- L40S: at 16K tokens, transformer latency 4.7× higher than at 1K tokens; Mamba maintains constant latency

**Critical weakness for SDD:** Mamba's weakness is in-context learning — the ability to adapt behavior based on information in the context window. For spec→code compilation, where the spec is in the context, this is a fundamental limitation. Transformer attention can precisely attend to arbitrary parts of the spec; Mamba compresses context into a fixed-size state vector and loses long-range dependencies.

Source: https://arxiv.org/pdf/2312.00752

**Verdict:** Mamba's determinism advantages don't compensate for in-context learning weakness. Code generation from specs is precisely the task where attention's arbitrary context access is critical.

### 8.2 Mixture of Experts (MoE) Non-Determinism

Many frontier models (GPT-4, Mixtral, DeepSeek) use MoE architectures where a subset of expert networks is activated per token. The routing decision is itself a stochastic element:

- Stochastic routing → unstable batching → fragmented workloads → poor reproducibility
- Without load balancing, models collapse to using only 2 experts
- Auxiliary losses for load balancing introduce additional hyperparameters that affect routing determinism

Recent work (ReMoE, ICLR 2025) replaces stochastic TopK routing with ReLU routing, which is differentiable and more deterministic at inference time. "Outperforms all methods including mainstream Top-K routing while benefiting from differentiability."

Source: https://arxiv.org/abs/2507.11181

**Verdict:** MoE routing is an additional non-determinism source on top of the kernel-level issues. Models with MoE architecture require batch-invariant fixes for both the expert routing and the kernel reduction paths.

### 8.3 Retrieval-Augmented Generation (RAG)

RAG injects retrieved context (relevant code examples, API docs, similar specs) into the prompt before generation.

**Determinism effect:**
- RAG retrieval is deterministic for identical queries (given fixed embedding model and vector store)
- The LLM generation step is still non-deterministic
- Net effect: RAG doesn't solve non-determinism but can improve spec→code quality by providing concrete examples to anchor generation. More anchored generation → less variance in practice.

**Verdict:** Useful for quality improvement and reducing effective variance, but not a determinism solution.

### 8.4 Neurosymbolic Hybrid: SymCode Pattern

SymCode (https://arxiv.org/abs/2510.25975) reframes generation as: LLM produces Python code using SymPy (a deterministic symbolic library); SymPy executes the code and provides exact results; errors trigger LLM refinement loops.

**Determinism properties:**
- SymPy execution is deterministic: same code → same output, always
- LLM generation of the SymPy program is still stochastic
- But the generated program is verified by execution before acceptance
- Self-debugging loop: if code fails to execute, LLM revises until it succeeds

**Results:** 13.6 percentage point accuracy improvement over pure LLM baselines on MATH-500 and OlympiadBench.

**For SDD:** This pattern — LLM generates, symbolic engine verifies, loop until verified — is directly applicable. The spec→code compilation can use:
1. LLM generates code
2. Test suite runs (deterministic execution)
3. Pass → accept and cache
4. Fail → LLM revises with error feedback
5. Repeat until pass or retry budget exhausted

This achieves **behavioral determinism** through test oracles, not inference determinism.

### 8.5 Decode-Verify-Rollback: LLM-42

LLM-42 (Microsoft Research, January 2026) is the most promising near-term architecture for selective determinism.

**Decode-Verify-Rollback (DVR) protocol:**
1. Fast path: standard autoregressive decoding with dynamic batching (fast, non-deterministic)
2. Verify window: replay the last N tokens under fixed-shape reduction schedules (same shape → same kernel path → deterministic)
3. Commit: if replay matches, tokens are committed
4. Rollback: if mismatch detected, revert to last committed token, regenerate

**Performance results (Llama-3.1-8B-Instruct, ShareGPT dataset):**

| Scenario | P50 Latency | P99 Latency |
|---|---|---|
| Non-deterministic baseline | 2.15s | 13.2s |
| SGLang-Deterministic (batch-invariant) | 4.64s | 28.0s |
| LLM-42 @ 2% deterministic traffic | 2.21s | ~13.5s |
| LLM-42 @ 100% deterministic traffic | 3.82s | ~19.0s |

| Deterministic traffic % | Throughput vs SGLang-Det |
|---|---|
| 10% | LLM-42 is 33-48% faster |
| 100% | LLM-42 is 6% slower (but within margin) |

**Key advantage:** Per-request `is_deterministic` flag. SDD compilation requests opt into determinism; other requests run at full speed. No global performance penalty.

**Recomputation overhead:** 0–10.97% depending on workload (rollback frequency). Over 50% of requests complete with zero rollbacks.

**Source:** https://arxiv.org/abs/2601.17768

**Status:** Research paper. Not yet integrated into production inference stacks. LLM-42 uses existing optimized kernels (cuBLAS, FlashAttention-3) unchanged, making integration tractable.

---

## 9. The "Good Enough" Threshold

### 9.1 Current Baseline (No Mitigation)

An empirical study of ChatGPT code generation (ACM TOSEM):
- 75.76% of coding tasks: zero identical test outputs across requests (HumanEval)
- 51.00% and 47.56% for two other benchmarks
- Maximum test pass rate difference of 1.00 (100%) for 39.63% of HumanEval problems
- Temperature=0 reduces but does not eliminate variance

Source: https://dl.acm.org/doi/10.1145/3697010

This is clearly unacceptable. At 75% variance rate, most rebuilds from spec produce different code.

**The logit gap mechanism explains why:** Non-determinism manifests at tokens where the probability gap between competing candidates is smaller than the accumulated numerical noise. Once a sequence diverges at one token, subsequent tokens diverge increasingly (compounding effect). The first 102 tokens of 1,000 identical runs can all match, then diverge from token 103 onward.

### 9.2 With Temperature=0 Only

From the "Non-Determinism of Deterministic LLM Settings" study (5 models, 10 runs each, MMLU + BIG-Bench Hard):
- GPT-3.5 Turbo: relatively stable
- Llama-3-70B-Instruct: frequently very low total agreement rates
- Mixtral-8x7B on college math: 72% performance difference between best and worst run
- Strong negative correlation between output length and stability

Source: https://arxiv.org/abs/2408.04667

Temperature=0 alone is necessary but not sufficient. The residual non-determinism is the batch-size-dependent kernel path issue.

### 9.3 With Batch-Invariant Kernels

SGLang deterministic mode (50 trials): single unique output in deterministic mode vs. 3-4 unique outputs in normal mode. The improvement is binary: with proper batch-invariant kernels, non-determinism is eliminated, not merely reduced.

### 9.4 The Retry Analysis

For a "95% of builds produce equivalent code" target:
- Detection: test oracle
- Handling: retry generation (but may produce another non-equivalent result)

If each generation independently has a 5% probability of producing non-equivalent code:
- 1 attempt: 95% pass rate
- 2 attempts: 99.75% pass rate
- 3 attempts: 99.99% pass rate

**However:** Failures are not independent. If a spec has an ambiguity that causes non-equivalent code, retrying with the same spec will likely produce another non-equivalent result. The fix is spec refinement, not retrying.

### 9.5 Practical Mitigation Tiers

**Tier 1: Eliminate non-determinism (high cost, requires infrastructure)**
- SGLang deterministic mode (self-hosted, H100/H200, +34%)
- TML batch_invariant_ops (+62%)
- LLM-42 per-request determinism (research, not production yet)
- FP32 inference instead of BF16 (+memory, no API support)

**Tier 2: Avoid non-determinism (medium cost, achievable today)**
- Exact-match content-addressable caching — cache first generation, replay forever
- Commit generated code to version control — treat first generation as ground truth
- Pin model versions to dated snapshots — prevent silent model updates
- Disable speculative decoding — removes one non-determinism source
- Disable KV cache quantization — removes precision-related variance

**Tier 3: Detect and handle non-determinism (low cost, always recommended)**
- Test oracles — run generated code against test suite; fail build on test failure
- Snapshot tests for generated code — compare against committed baseline
- Semantic equivalence checking — compare new generation against committed version (expensive)
- Retry with spec refinement — use test failure messages to improve the spec

**Tier 4: Statistical (acceptable for research, not production)**
- Pass@k: generate N times, keep best according to tests
- Majority voting: generate N times, select most common
- Uncertainty estimation: quantify confidence before deploying

### 9.6 Prompt Snapshot Testing

An emerging practice for managing LLM output regression in CI/CD:
- Generate JSON snapshots of prompt → output pairs
- Commit snapshots to version control alongside prompts
- CI checks: if snapshot differs from committed, flag for human review

Source: https://ninkovic.dev/blog/2025/prompt-snapshot-testing

**For SDD:** Snapshot tests work well when the generated code is committed to VCS (which it should be). They detect regressions when regenerating code after spec changes. The limitation: non-determinism means even unchanged specs can produce snapshot mismatches, requiring manual review of "spurious" failures.

---

## 10. Synthesis: Implications for CodeSpeak and SpecPunk

### 10.1 The Core Problem Statement (Refined)

Non-determinism in spec→code compilation has two regimes:

**Regime 1: Infrastructure non-determinism** (batch-size-dependent kernel paths)
- Affects greedy decoding (temperature=0) even with fixed seeds
- Cannot be controlled through API parameters today
- Controllable with SGLang deterministic mode (self-hosted only) or batch-invariant kernels
- For production SDD tools targeting API-based inference: intractable until providers ship batch-invariant kernels

**Regime 2: Semantic non-determinism** (spec ambiguity amplified by generation variance)
- Affects all temperatures, all inference configurations
- Controllable through spec precision improvement
- Detectable through test oracles
- Manageable through exact-match caching of verified generations

### 10.2 Recommended Architecture for SpecPunk / CodeSpeak

Given the state of the art in March 2026, the optimal pipeline is:

```
Compilation pipeline:
1. Compute: cache_key = sha256(spec || model_version || system_prompt)
2. Lookup cache_key in content-addressable store
   - Hit:  return cached code (bitwise deterministic forever)
   - Miss: proceed to generation
3. Generate(spec, model=pinned_version, temp=0, seed=fixed)
4. Run test suite against generated code
   - Pass:  store in cache, commit to VCS → deterministic going forward
   - Fail:  log failure, refine spec, retry (up to N times)
5. All subsequent builds hit cache → zero non-determinism
```

This architecture achieves:
- **Bitwise determinism** after first successful generation (via caching)
- **Semantic correctness verification** via test suite
- **Team consistency** — everyone reads from the same cache
- **Debugging reproducibility** — committed code is immutable
- **CI/CD reproducibility** — CI hits cache, not live inference

### 10.3 On the 62% Overhead Framing

The 62% overhead should be read as:
- Baseline: 26 seconds for 1,000 sequences (throughput benchmark)
- Deterministic: 42 seconds for 1,000 sequences

For SDD use cases: a single spec compilation typically takes 2–30 seconds regardless. A 62% overhead on an operation performed once (with results cached indefinitely) is trivially acceptable. The overhead matters for **high-concurrency real-time inference serving**, not for infrequent spec compilation.

The SGLang 34% overhead (better than TML's 62%) further reduces the cost of deterministic compilation for self-hosted deployments.

### 10.4 The Model Deprecation Problem Requires a Migration Protocol

Any SDD tool must treat model version as a first-class build parameter. When a model is deprecated:
1. Trigger regeneration for all specs using the deprecated model
2. Run full test suite against all newly generated artifacts
3. Review semantic diffs (code changes not justified by spec changes = regression)
4. Update the cache with newly generated artifacts

This migration must be treated as a breaking change with a testing gate, not a silent upgrade.

### 10.5 Open Research Questions for SpecPunk

1. **What is the actual non-determinism rate for spec→code generation specifically?** No study has measured this for real SDD specs (as opposed to HumanEval-style benchmarks). A controlled experiment with real CodeSpeak/Tessl specs would provide ground truth.

2. **Does spec precision have a monotonic relationship with generation consistency?** The Böckeler observation suggests yes, but there is no systematic study with quantitative data.

3. **Is there a minimal spec length/precision threshold below which generation variance is unacceptable for production?** This would define spec quality requirements.

4. **Can semantic caching hit rates be made acceptable for spec→code?** Given high mismatch costs in code generation, conservative similarity thresholds likely make exact-match caching more practical than semantic caching for this domain.

5. **When does LLM-42's selective per-request determinism mode become available via public APIs?** If Microsoft ships this to Azure OpenAI, it would be the first production-grade deterministic inference API available to SDD tools without self-hosting.

6. **Does Clover's closed-loop verification approach generalize beyond Dafny-expressible programs?** A version using test suites instead of formal proofs would be directly applicable to SDD.

---

## References

### Primary Research

| Source | URL |
|---|---|
| TML: Defeating Nondeterminism in LLM Inference (Nov 2025) | https://thinkingmachines.ai/blog/defeating-nondeterminism-in-llm-inference/ |
| batch_invariant_ops (code) | https://github.com/thinking-machines-lab/batch_invariant_ops |
| NeurIPS 2025: Numerical Sources of Nondeterminism | https://arxiv.org/abs/2506.09501 |
| LLM-42: Determinism via Verified Speculation (Jan 2026) | https://arxiv.org/abs/2601.17768 |
| SGLang Deterministic Inference (Sep 2025) | https://lmsys.org/blog/2025-09-22-sglang-deterministic/ |

### Non-Determinism Taxonomy and Measurement

| Source | URL |
|---|---|
| Non-Determinism of Deterministic LLM Settings (2025) | https://arxiv.org/abs/2408.04667 |
| ACM TOSEM: Non-Determinism of ChatGPT in Code Generation | https://dl.acm.org/doi/10.1145/3697010 |
| NAACL 2025: Evaluation Should Not Ignore Non-Determinism | https://aclanthology.org/2025.naacl-long.211/ |
| Simon Willison commentary | https://simonwillison.net/2025/Sep/11/defeating-nondeterminism/ |
| LLM Watch ELI5 | https://www.llmwatch.com/p/eli5-defeating-nondeterminism-in |

### API Determinism Evidence

| Source | URL |
|---|---|
| OpenAI Reproducible Outputs Cookbook | https://cookbook.openai.com/examples/reproducible_outputs_with_the_seed_parameter |
| Claude Code non-determinism bug report (Jul 2025) | https://github.com/anthropics/claude-code/issues/3370 |
| Gemini 2.5 Pro non-determinism with seed (2025) | https://discuss.ai.google.dev/t/the-gemini-api-is-exhibiting-non-deterministic-behavior-for-the-gemini-2-5-pro-model/101331 |
| Is Zero Temperature Deterministic? (Google Cloud) | https://medium.com/google-cloud/is-a-zero-temperature-deterministic-c4a7faef4d20 |
| How to get consistent LLM outputs in 2025 | https://www.keywordsai.co/blog/llm_consistency_2025 |

### Caching and Memoization

| Source | URL |
|---|---|
| GPTCache (GitHub) | https://github.com/zilliztech/GPTCache |
| GPT Semantic Cache paper (Nov 2024) | https://arxiv.org/abs/2411.05276 |
| Semantic Caching for Low-Cost LLM Serving (Aug 2025) | https://arxiv.org/abs/2508.07675 |
| LMCache enterprise KV cache | https://arxiv.org/abs/2510.09665 |
| Propel: implications for engineering teams | https://www.propelcode.ai/blog/defeating-nondeterminism-in-llm-inference-ramifications |
| Prompt snapshot testing | https://ninkovic.dev/blog/2025/prompt-snapshot-testing |

### Equivalence Checking and Formal Verification

| Source | URL |
|---|---|
| EquiBench (EMNLP 2025) | https://arxiv.org/abs/2502.12466 |
| Clover: Closed-Loop Verifiable Code Generation (SAIV 2024) | https://arxiv.org/abs/2310.17807 |
| SymCode: Neurosymbolic Verifiable Code Generation | https://arxiv.org/abs/2510.25975 |

### Spec-Driven Development

| Source | URL |
|---|---|
| Martin Fowler: Kiro, spec-kit, and Tessl (Mar 2026) | https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html |
| Tessl: how its products pioneer SDD | https://tessl.io/blog/how-tessls-products-pioneer-spec-driven-development/ |
| Tessl launch announcement | https://tessl.io/blog/tessl-launches-spec-driven-framework-and-registry/ |
| CodeSpeak: codespeak takeover 2026 | https://codespeak.dev/blog/codespeak-takeover-20260223 |
| SDD Triangle (dbreunig, Mar 2026) | https://www.dbreunig.com/2026/03/04/the-spec-driven-development-triangle.html |
| Spec-Driven Development: arxiv paper (Feb 2026) | https://arxiv.org/abs/2602.00180 |

### Alternative Architectures

| Source | URL |
|---|---|
| Mamba: Linear-Time Sequence Modeling | https://arxiv.org/pdf/2312.00752 |
| MoE: Mixture of Experts survey 2025 | https://arxiv.org/abs/2507.11181 |
| RAG for Code Generation survey | https://arxiv.org/abs/2510.04905 |
| ReMoE: differentiable MoE routing (ICLR 2025) | https://proceedings.iclr.cc/paper_files/paper/2025/file/94dc604e115237a7f4a758b3146cd976-Paper-Conference.pdf |

### Reproducibility in SE Research

| Source | URL |
|---|---|
| LLMs for SE: A Reproducibility Crisis | https://arxiv.org/abs/2512.00651 |
| Model versioning and rollback plan | https://www.rohan-paul.com/p/plan-for-versioning-and-potentially-rolling-back-an-llm-deployment |
| Automated version control with DVC + CI/CD | https://circleci.com/blog/automated-version-control-for-llms-using-dvc-and-ci-cd/ |

---

*Research completed: 2026-03-12. Knowledge cutoff for sources: March 2026. All URLs accessed March 2026.*
