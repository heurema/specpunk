---
title: "Consistency Checking for Natural Language Specifications"
date: 2026-03-12
context: CodeSpeak / Breslav "type system for NL" — catching terminology inconsistency, logical contradictions, module reference integrity, gap detection
tags: [requirements-engineering, NLP, consistency-checking, LLM, specifications, type-systems]
status: done
---

# Consistency Checking for Natural Language Specifications

Research question: what exists for building a "type system for NL specifications" — catching terminology inconsistency, logical contradictions, module reference integrity, and spec gaps?

## 1. NLP-Based Contradiction Detection in Requirements

### NLI (Natural Language Inference) for requirements

Natural Language Inference reformulates contradiction detection as an entailment problem: given two sentences, classify the relationship as entailment / contradiction / neutral. Applied to requirements:
- Outperforms classical NLP methods and LLM chatbots for requirements classification, defect identification, and conflict detection (Cabrera 2024, arxiv 2405.05135)
- Works well for pairwise contradiction (requirement A contradicts requirement B)
- Critical limitation: NLI cannot detect **composite conflicts** — inconsistencies that only emerge from three or more interdependent requirements

### ALICE — Automated Logic for Identifying Contradictions in Engineering (2024)

System by Gärtner & Göhlich (TU Berlin), published in *Automated Software Engineering* (Springer, 2024).

Architecture:
- LLM translates NL requirements into formal logic representation
- Decision tree with 7 questions classifies contradiction type
- Formal solver verifies the logic layer
- Expanded taxonomy of contradiction types

Results on real-world requirements datasets:
- 99% accuracy
- 60% recall (detects 60% of all contradictions — LLM-only approaches detect significantly fewer)
- Hybrid approach (formal + LLM) markedly surpasses LLM-only

Key insight: pure LLM → high precision, low recall. Formal logic layer catches what the LLM misses. Recall ceiling at 60% is the current practical frontier.

Source: https://link.springer.com/article/10.1007/s10515-024-00452-x

### LLM-based requirements verification (2024)

Study by arxiv 2411.11582 — LLMs assess whether system specifications fulfill requirements. Ground truth: SysML/OCL formal models.

Results:
- GPT-4o and Claude 3.5 Sonnet: F1-score 79–94% for identifying unfulfilled requirements
- GPT-3.5-turbo: ~50% F1-score (not viable)
- Chain-of-thought + few-shot prompting improve results significantly
- Structured, concise spec language (vs. verbose prose) improves LLM accuracy
- Fewer requirements per prompt → better results

Practical limit: LLMs underperform formal methods but have zero setup cost and no schema requirement.

Source: https://arxiv.org/html/2411.11582v1

### Requirement smells — Paska tool (FSE 2024)

Paska: automated quality smell detection for NL requirements. Pipeline:
- Tokenization, lemmatization, POS tagging, constituency parsing, glossary search, Tregex
- Maps requirements to Rimay CNL patterns (Controlled Natural Language for requirements)
- Detects: ambiguity, incompleteness, atomicity violations, correctness issues

Evaluation on 2,725 annotated requirements from 13 financial systems:
- 89% precision and recall for smell detection
- 96% precision / 94% recall for Rimay pattern recommendations

Source: https://arxiv.org/abs/2305.07097

### Multi-label requirement smell classification (2025, Nature Scientific Reports)

11 smell types from ISO/IEC/IEEE 29148:
- Subjective language, comparative/superlative phrases
- Passive voice, uncertain verbs, ambiguous adverbs
- Negative statements, polysemy, vague pronouns
- Open-ended non-verifiable terms, loopholes

Deep learning: Bi-LSTM + ELMo embeddings → 90.3% F1 macro on 8,120 requirements.

Relevant to CodeSpeak: polysemy and vague pronouns map directly to "terminology inconsistency" (same word used with different meanings in different modules).

Source: https://pmc.ncbi.nlm.nih.gov/articles/PMC11833090/

### Automotive toolchain (RWTH Aachen)

NLP consistency-checking toolchain for automotive requirements:
- Structured pipeline: grammar parsing → semantic extraction → consistency check
- Detects inter-requirement conflicts within controlled domains
- Published as an industrial application

Source: https://www.se-rwth.de/publications/Leveraging-Natural-Language-Processing-for-a-Consistency-Checking-Toolchain-of-Automotive-Requirements.pdf

---

## 2. DDD Automation — Ubiquitous Language and Terminology Drift

### State of the art

DDD ubiquitous language enforcement remains largely manual. The concept (Evans 2003): all stakeholders share a single glossary, implemented in code. Drift = the same concept referred to by different names across bounded contexts, or the same name meaning different things.

No dedicated open-source tool for automated glossary extraction and drift detection was found. The gap is real and acknowledged.

### What exists

**NLP-based entity extraction from domain specs** — extracting nouns-as-entities, verbs-as-relationships, determiners-for-cardinality from NL text. Multiple academic systems generate ER models from requirements text (NADIAPUB 2019, dblp/Maltemp 2024). Tools generate domain entity graphs but do not enforce consistency across bounded contexts.

**Coreference resolution for requirements** — detecting when two different linguistic expressions refer to the same entity (Springer RE journal 2022). Example: "customer" and "the buyer" as coreferents. Tool detects them; enforcement (making the author choose one canonical term) is not automated.

Source: https://link.springer.com/article/10.1007/s00766-022-00374-8

**Cross-domain ambiguity detection** — NLP approach ranks terms by ambiguity score based on differences in word embeddings between domain-specific language models (Springer ASE, 2019). Identifies terms that mean different things across bounded contexts.

Source: https://link.springer.com/article/10.1007/s10515-019-00261-7

**LLM-based ontology extraction** (2024–2025):
- OntoKGen (arxiv 2412.00608): chain-of-thought pipeline → ontology → knowledge graph in Neo4j. Interactive, user-guided. Targeted at unstructured technical documents.
- Llm-empowered KG construction survey (arxiv 2510.20345): broad review of extraction approaches including embedding-based deduplication for terminology merging.

**GLaMoR — graph language models for ontology consistency checking** (2025):
- Converts OWL ontologies to triple sequences (S-R-O)
- T5-based GLM detects 14 anti-patterns (cyclic hierarchies, disjointness violations, domain/range conflicts)
- 95.13% accuracy, ~20x faster than HermiT reasoner
- Relevant: once terms are extracted into an ontology, GLaMoR can check it for logical contradictions

Source: https://arxiv.org/html/2504.19023v1

### Practical recommendation for CodeSpeak-style checking

Pipeline that could work today:
1. Extract glossary (NER + noun-phrase extraction from spec modules)
2. Run coreference resolution to detect synonym aliases
3. Embed terms with domain-specific embeddings, cluster by similarity → flag near-duplicates with different spellings as potential terminology drift
4. Cross-module comparison: same term, different definition (polysemy detection)

No single off-the-shelf tool does all four steps for NL specs. Has to be assembled.

---

## 3. "Type Systems" for Informal Specifications

### Clover — consistency as the proxy for correctness (Stanford, SAIV 2024)

Core insight: **reduce correctness checking to consistency checking**. Three artifacts must be mutually consistent:
1. Code
2. Docstring (NL description)
3. Formal annotation (Dafny spec)

Six pairwise consistency checks. If all three agree, correctness is highly likely.

Results on CloverBench (Dafny textbook programs):
- Up to 87% acceptance rate for correct instances
- Zero false positives on adversarial incorrect instances
- Detected 6 incorrect programs in human-written MBPP-DFY-50

The analogy to CodeSpeak: spec ↔ docstring, generated code ↔ code, intermediate representation ↔ formal annotation. Consistency among three layers is checkable without ground truth.

Source: https://arxiv.org/abs/2310.17807 / https://ai.stanford.edu/blog/clover/

### What can be statically checked in NL

The Breslav framing (from CodeSpeak blog and Pragmatic Engineer interview): CodeSpeak is more like Python (dynamic) than Kotlin (static) because LLMs require NL input. But certain checks are tractable without full formalization:

| Check type | Mechanism | Difficulty |
|---|---|---|
| Undefined term reference | Glossary lookup — term used but not defined | Low (lexical) |
| Term redefinition | Same term, different definition in different modules | Low-medium (compare embeddings) |
| Synonym proliferation | Two terms, same definition | Medium (coreference/embedding) |
| Logical contradiction | A requires X; B forbids X | High (NLI or formal logic) |
| Missing module reference | "See authentication module" but no such module | Low (string/ID match) |
| Cardinality conflict | "one user" vs "many users" for same entity | Medium (dependency parse) |
| Gap / underspecification | Required behavior for case C not mentioned anywhere | Hard (coverage analysis) |

Entity/relationship extraction as lightweight types: nouns become entity types, verbs become relationship types. Once extracted, type-level checks become feasible (undefined type used in relationship, duplicate type in same module, etc.).

### Controlled Natural Language approach

Restricting requirements to a CNL (controlled natural language) unlocks formal checking. SBVR (Semantics of Business Vocabularies and Rules) and Rimay (used by Paska) are examples. Trade-off: expressiveness for checkability. More like Kotlin — statically safe but more ceremony.

---

## 4. Gap Detection and Smart Disambiguation

### Gap detection — what LLMs will guess wrong

The core problem: spec has cases A, B but not C. LLM will infer C by analogy — sometimes correctly, sometimes catastrophically. Gap detection = identifying where the spec is silent.

Approaches found:

**LHAW controllable underspecification** (arxiv 2602.10525): decomposes underspecification across four dimensions — Goals, Constraints, Inputs, Context. Generates controlled underspecified variants to test agent clarification behavior. Framework for identifying what type of information is missing.

**Completeness checking with LLMs** (ISO 29148 approach): Llama 2 (70B) gives binary evaluations per quality characteristic (completeness, singularity, verifiability) with rationales. Practical but requires human review for final judgment.

**Checklist-based gap analysis**: GPT-4o as checklist creator (5–10 independent, relevant, non-redundant questions per requirement). Each checklist item is a targeted probe for missing information. 2025 research confirms independence of questions is critical for complete evaluation coverage.

**Requirements elicitation follow-up questions** (IEEE RE 2025, arxiv 2507.02858):
- LLM generates follow-up questions for requirements elicitation interviews
- Guided generation (based on common interviewer mistake types) outperforms human-authored questions
- Unguided generation matches human quality
- Real-time use: reduces cognitive load during elicitation sessions

Source: https://arxiv.org/html/2507.02858v1

### Multiple-choice vs. open-ended clarification

**AmbiSQL** (arxiv 2508.15276) — most concrete model found for targeted multiple-choice clarification:

Architecture:
- Stage 1: LLM detects ambiguous phrases, classifies them (DB-related vs. LLM-reasoning-related)
- Generates targeted multiple-choice clarification questions with relevant schema snippets
- Stage 2: user selects answer → query rewritten → re-checked for new ambiguities (iterative)
- Tree structure indexes stored clarifications for reuse

Results on ambiguous NL-to-SQL queries:
- 87.2% precision / 89.1% recall / 88.2% F1 for ambiguity detection
- SQL accuracy: 42.5% → 92.5% (exact match) after clarification
- BIRD benchmark: 75% → 100%

Key pattern: **classify ambiguity type first, then generate targeted question**. Generic open-ended clarification is less effective than structured multiple-choice tied to the specific ambiguity type.

Source: https://arxiv.org/html/2508.15276

### Confidence scoring for gap/ambiguity detection

From 2024–2025 research:
- ASK4CONF: instructs LLM to directly produce aleatoric uncertainty as "clarity probability" → threshold determines whether to proceed or ask
- Conformal prediction framework: normalized probability scores → prediction set; if only one action remains after confidence filtering → execute; if multiple → request human input
- Calibration research (ICLR 2025): LLMs are systematically overconfident; conformal calibration reduces miscalibration
- CLAMBER benchmark: taxonomy of ambiguity types — lexical ambiguity, semantic underspecification, epistemic uncertainty

Practical pattern for CodeSpeak-style gap detector:
1. For each spec section, compute confidence of LLM interpretation (multiple sampling or logprob-based)
2. Low confidence → identify ambiguity type from CLAMBER taxonomy
3. Generate targeted multiple-choice question specific to that ambiguity type
4. Iterate until confidence above threshold

---

## 5. Practical Tools — Current Landscape

### Prose / spec linters

| Tool | Type | What it checks | Relevant for NL specs |
|---|---|---|---|
| Vale (vale.sh) | Rule-based prose linter | Style consistency, terminology, custom rules via YAML | Yes — can encode glossary rules, forbidden synonyms, required patterns |
| textlint | Pluggable NL linter | Customizable rules for prose quality | Medium — requires plugin authoring |
| Paska | Requirements-specific | 89% precision on smell detection, Rimay CNL compliance | High — purpose-built for requirements |
| Spectral | API spec linter | OpenAPI / JSON Schema consistency | Low — structured specs only |

**Vale** is the most immediately deployable: YAML-based rules can check for forbidden synonyms, require defined terms, flag undefined references by name. Works as a CI gate. Not semantic — purely lexical — but catches large class of terminology drift.

Source: https://vale.sh

### Contradiction detectors

| Tool | Approach | Accuracy | Production-ready |
|---|---|---|---|
| ALICE | Formal logic + LLM | 99% accuracy, 60% recall | Research prototype |
| NLI classifiers (RoBERTa, DeBERTa) | Fine-tuned NLI | Varies by domain | Deployable via HuggingFace |
| GPT-4o / Claude direct | Pure LLM | F1 79–94% | Yes, but expensive per check |

No off-the-shelf contradiction detector for NL specs is production-packaged. ALICE methodology is the best academic baseline to implement.

### Ontology / terminology consistency

| Tool | Approach | Notes |
|---|---|---|
| Protégé | OWL ontology editor + reasoner | Full formal ontology; HermiT reasoner for consistency |
| Fluent Editor | Controlled NL → OWL | CNL-based, makes ontology authoring accessible |
| GLaMoR | Graph LM for OWL consistency | 95% accuracy, 20x faster than HermiT |
| OntoKGen | LLM → KG pipeline | CoT-based extraction, Neo4j output |

For a CodeSpeak-style workflow: Fluent Editor (CNL input) → GLaMoR (consistency check) is a viable formal path. High ceremony but strong guarantees.

### Gap analyzers

No dedicated open-source "spec gap analyzer" exists. Closest:
- **LHAW**: research framework for generating underspecified variants — useful for testing, not production gap detection
- **LLM completeness checkers**: completeness evaluation per ISO 29148 standard, via prompt templates
- **AmbiSQL clarification engine**: most concrete reference implementation for interactive gap resolution

### Modular interface pattern

A "spec type checker" with modular interfaces, analogous to a compiler pipeline:

```
spec_modules/
├── lexer/         → tokenize, extract terms, build glossary
├── resolver/      → coreference, cross-module reference integrity
├── type_checker/  → entity type consistency, relationship consistency
├── contradiction/ → NLI pairwise + ALICE-style formal check
├── gap_detector/  → coverage analysis, confidence-based probing
└── reporter/      → structured output, targeted MCQ generation
```

Each stage has a defined input (NL text + module graph) and output (typed issues with location + severity + suggested fix or clarification question).

---

## Key Findings Summary

1. **Contradiction detection**: ALICE (hybrid formal+LLM) is the best known method — 60% recall, 99% accuracy. Pure LLM achieves 79–94% F1 but misses non-pairwise contradictions. NLI covers pairwise only.

2. **Terminology / DDD**: No production tool for automated ubiquitous language enforcement. Combination of coreference resolution + domain embedding comparison + cross-module NER covers most cases. GLaMoR covers ontology-level consistency once terms are formalized.

3. **Type system analogy**: The Clover insight is the most powerful — reduce correctness to consistency among three mutually checked artifacts. For NL specs: (NL spec module) × (generated code) × (formal annotation). Entity/relationship extraction gives lightweight types. CNL (Rimay, SBVR) gives full static checking at cost of expressiveness.

4. **Gap detection**: AmbiSQL is the best reference for interactive multiple-choice clarification (87% detection F1, near-doubling task accuracy). Confidence thresholds (ASK4CONF, conformal prediction) determine when to ask. Classify ambiguity type first, then generate targeted question.

5. **Practical tools**: Vale for terminology linting (deployable now), Paska for smell detection (research code), NLI models via HuggingFace for contradiction (deployable, domain-tuning needed), AmbiSQL pattern for clarification (implement from paper). No single packaged tool covers the full pipeline.

---

## Sources

- ALICE (contradiction detection): https://link.springer.com/article/10.1007/s10515-024-00452-x
- NLI in RE (Cabrera 2024): https://arxiv.org/abs/2405.05135
- LLM spec verification (arxiv 2411.11582): https://arxiv.org/html/2411.11582v1
- Paska smell detection (FSE 2024): https://arxiv.org/abs/2305.07097
- Multi-label requirement smells (2025): https://pmc.ncbi.nlm.nih.gov/articles/PMC11833090/
- Clover closed-loop verification (Stanford SAIV 2024): https://arxiv.org/abs/2310.17807
- AmbiSQL interactive clarification: https://arxiv.org/html/2508.15276
- GLaMoR ontology consistency: https://arxiv.org/html/2504.19023v1
- OntoKGen (LLM ontology pipeline): https://arxiv.org/abs/2412.00608
- Coreference in requirements (Springer RE 2022): https://link.springer.com/article/10.1007/s00766-022-00374-8
- Cross-domain ambiguity detection (Springer ASE 2019): https://link.springer.com/article/10.1007/s10515-019-00261-7
- Requirements elicitation follow-up questions (IEEE RE 2025): https://arxiv.org/html/2507.02858v1
- LLM gap completeness + research directions (Frontiers 2025): https://www.frontiersin.org/journals/computer-science/articles/10.3389/fcomp.2025.1519437/full
- LHAW underspecification (arxiv 2602.10525): https://arxiv.org/html/2602.10525
- NL requirements ambiguity (industrial study): https://www.ipr.mdu.se/pdf_publications/7221.pdf
- Requirement smell (NL requirements, comparison study): https://www.researchgate.net/publication/364305559_Using_NLP_Tools_to_Detect_Ambiguities_in_System_Requirements-A_Comparison_Study
- NLP for RE (systematic mapping): https://dl.acm.org/doi/10.1145/3444689
- Vale prose linter: https://vale.sh
- CodeSpeak: https://codespeak.dev
- Clover SAIL Blog: https://ai.stanford.edu/blog/clover/
