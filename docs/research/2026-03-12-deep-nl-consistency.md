---
title: "NL Consistency Checking & Type Systems for Natural Language Specs — Deep Dive"
date: 2026-03-12
pass: 2
context: CodeSpeak / specpunk — second pass, avoiding all content from 2026-03-12-nl-consistency-checking.md
tags: [requirements-engineering, NLP, consistency-checking, LLM, CNL, knowledge-graph, spec-linter, gap-detection, contract-testing]
status: done
---

# NL Consistency Checking & Type Systems for Natural Language Specs — Deep Dive (Pass 2)

This document is a second-pass deep research. It does NOT repeat findings from the initial survey (`2026-03-12-nl-consistency-checking.md`). It covers concrete implementation details, formal method internals, graph-based approaches, LLM cost analysis, gap taxonomy, cross-module contracts, and real-world defect data.

---

## 1. Building a Spec Linter — Concrete Implementation

### 1.1 Vale Rule Architecture for Spec Checking

Vale (Go, MIT, v3.13.1 as of Feb 2026) is a markup-aware prose linter with YAML-defined rules. It is the fastest open-source prose linter: outperforms proselint (Python) and write-good (Node.js) on all tested corpora, measured with `hyperfine` on MacBook Pro 2.9 GHz i7.

**Check types relevant to spec linting:**

**`existence`** — flag forbidden tokens (vague verbs, ambiguous quantifiers):

```yaml
# rules/SpecQuality/VagueVerbs.yml
extends: existence
message: "'%s' is too vague for a spec — prefer a precise verb (return, emit, store, reject)"
level: error
ignorecase: true
tokens:
  - process
  - handle
  - manage
  - deal with
  - support
```

**`substitution`** — enforce canonical terminology (the core of a spec glossary):

```yaml
# rules/SpecQuality/CanonicalTerms.yml
extends: substitution
message: "Use '%s' instead of '%s' (per spec glossary section 2.1)"
level: warning
ignorecase: true
swap:
  "user account": "account"
  "end-user": "user"
  "client application": "client"
  "auth token": "token"
  "authentication token": "token"
```

**`consistency`** — catch a term used inconsistently with itself across a document:

```yaml
# rules/SpecQuality/TermConsistency.yml
extends: consistency
message: "Use '%s' or '%s' consistently — do not mix both"
level: warning
ignorecase: false
either:
  "payload": "body"
  "endpoint": "route"
  "response code": "status code"
```

**`occurrence`** — flag when a sentence has more than one actor (atomicity check):

```yaml
# rules/SpecQuality/MultipleActors.yml
extends: occurrence
message: "Sentence contains multiple actors — split into atomic requirements"
level: warning
scope: sentence
max: 1
token: '\b(the system|the user|the client|the server)\b'
```

**Vocabulary (glossary integration):**
Place `accept.txt` and `reject.txt` in `styles/config/vocabularies/SpecGlossary/`:
- `accept.txt`: canonical terms (e.g., `token`, `account`, `endpoint`) — Vale will not flag these as misspellings or substitutions
- `reject.txt`: banned terms (e.g., `blob`, `stuff`, `thing`) — triggers `Vale.Avoid` rule automatically

**`.vale.ini` for a spec project:**
```ini
StylesPath = .vale/styles
MinAlertLevel = warning

[formats]
md = markdown
txt = markdown

[*.md]
BasedOnStyles = Vale, SpecQuality
SpecQuality.VagueVerbs = YES
SpecQuality.CanonicalTerms = YES
SpecQuality.TermConsistency = YES
SpecQuality.MultipleActors = YES
Vale.Spelling = YES
```

### 1.2 NLI Models as CI Check

**Model selection:** `cross-encoder/nli-deberta-v3-base` (HuggingFace) — trained on SNLI + MultiNLI, outputs three scores: `contradiction`, `entailment`, `neutral`. SNLI test accuracy: 92.38%, MNLI mismatched: 90.04%.

**Implementation pattern for pairwise contradiction scan:**

```python
# ci/check_spec_consistency.py
from sentence_transformers import CrossEncoder
from itertools import combinations
import json, sys

model = CrossEncoder('cross-encoder/nli-deberta-v3-base')
label_mapping = ['contradiction', 'entailment', 'neutral']

def load_requirements(path: str) -> list[dict]:
    """Load requirements from a JSONL or markdown file."""
    reqs = []
    with open(path) as f:
        for line in f:
            if line.strip().startswith('- '):
                reqs.append({'id': f"R{len(reqs)+1}", 'text': line.strip()[2:]})
    return reqs

def check_contradictions(reqs: list[dict], threshold: float = 0.5) -> list[dict]:
    pairs = list(combinations(reqs, 2))
    sentences = [(a['text'], b['text']) for a, b in pairs]
    scores = model.predict(sentences)

    results = []
    for (a, b), score_vec in zip(pairs, scores):
        label = label_mapping[score_vec.argmax()]
        contradiction_score = score_vec[0]  # index 0 = contradiction
        if label == 'contradiction' or contradiction_score > threshold:
            results.append({
                'req_a': a['id'], 'req_b': b['id'],
                'score': float(contradiction_score),
                'text_a': a['text'], 'text_b': b['text']
            })
    return sorted(results, key=lambda x: -x['score'])

if __name__ == '__main__':
    reqs = load_requirements(sys.argv[1])
    issues = check_contradictions(reqs)
    if issues:
        print(json.dumps(issues, indent=2))
        sys.exit(1)  # fail CI
    sys.exit(0)
```

**GitHub Actions integration:**
```yaml
# .github/workflows/spec-check.yml
- name: Check spec consistency
  run: |
    pip install sentence-transformers torch
    python ci/check_spec_consistency.py docs/spec/requirements.md

- name: Vale prose lint
  uses: errata-ai/vale-action@reviewdog
  with:
    files: docs/spec/
    version: 3.13.1
```

**False positive thresholds in practice:** The NLI model generates false positives for requirements that share terminology but operate in different contexts (e.g., "The system SHALL validate tokens on login" vs "The system SHALL reject expired tokens"). In testing on requirements corpora, a threshold of 0.65 for contradiction score reduces false positives significantly while catching genuine conflicts. Teams at LegalLens 2024 achieved F1 = 84.73% on domain-adapted NLI tasks by fine-tuning DeBERTa with domain data augmentation.

### 1.3 Architecture: CLI vs LSP vs IDE Plugin

Three deployment modes with different trade-offs:

| Mode | Latency | Integration Cost | User Disruption | Best For |
|------|---------|-----------------|-----------------|----------|
| Standalone CLI | 100–500ms for 1K lines | Low — one binary | None (batch) | CI gate |
| Language Server (LSP) | 50–200ms incremental | Medium — client required | Inline squiggles | Active writing |
| IDE Plugin | 10–50ms (cached) | High — per-IDE | Inline + refactor | Power users |

**LSP architecture for a spec linter:**

The Language Server Protocol (LSP) is now supported by all major IDEs: VS Code, Neovim, IntelliJ (since 2023.2). An LSP server receives `textDocument/didChange` notifications and publishes `textDocument/publishDiagnostics` with a list of ranges and messages.

Minimal LSP wrapper pattern (Python, using `pygls`):
```python
from pygls.server import LanguageServer
from lsprotocol.types import (
    TEXT_DOCUMENT_DID_CHANGE, DidChangeTextDocumentParams,
    Diagnostic, DiagnosticSeverity, Range, Position
)

server = LanguageServer("spec-linter", "v0.1")

@server.feature(TEXT_DOCUMENT_DID_CHANGE)
def on_change(ls: LanguageServer, params: DidChangeTextDocumentParams):
    text = params.content_changes[0].text
    diagnostics = run_checks(text)  # Vale + NLI + graph
    ls.publish_diagnostics(params.text_document.uri, diagnostics)

server.start_io()
```

Vale itself does not ship an LSP server, but the `vale-ls` project (community) wraps it. For a custom spec linter, running Vale as a subprocess and parsing its JSON output is the simplest approach:

```bash
vale --output=JSON spec.md | jq '.[] | .[] | {line: .Line, message: .Message}'
```

**Performance for a 1,000-line spec:**
- Vale (Go binary, pre-compiled rules): ~80–150ms total, including startup
- DeBERTa NLI (O(n²) pairs, n=200 requirements): 2–8 seconds on CPU, ~300ms on GPU
- The O(n²) complexity of pairwise NLI is the practical bottleneck — mitigated by batching (CrossEncoder supports batch_size=64) and pre-filtering with fast semantic similarity to only check pairs above a cosine threshold.

Pre-filter pattern to limit NLI pairs:
```python
from sentence_transformers import SentenceTransformer
import numpy as np

embedder = SentenceTransformer('all-MiniLM-L6-v2')
embeddings = embedder.encode([r['text'] for r in reqs])
similarity_matrix = np.dot(embeddings, embeddings.T)

# Only run NLI on pairs with cosine similarity > 0.3 (related enough to potentially conflict)
candidate_pairs = [
    (i, j) for i in range(len(reqs)) for j in range(i+1, len(reqs))
    if similarity_matrix[i, j] > 0.3
]
# Reduces pairs from O(n²) to ~O(n·k) where k is average related requirements
```

---

## 2. Formal Methods for Natural Language

### 2.1 SBVR — Deep Dive

**What it is:** SBVR (Semantics of Business Vocabulary and Business Rules), OMG standard v1.5 (current), first adopted 2008. Maps to a formalization of classical logic, specifically to modal logic with deontic operators (obligation, permission, prohibition).

**Technical foundation:**
- Uses OMG Meta-Object Facility (MOF) for interchange (MOF/XMI mapping)
- Generates XML schemas for tool interchange
- Linked to Knowledge Discovery Metamodel (KDM) for software analytics

**What SBVR can express:**
- Business concept definitions with formal semantics
- Necessity (must), possibility (may), obligation, prohibition operators
- Quantification over business concepts
- Constraint expressions over facts

**What SBVR cannot express:**
- Procedural sequences (it is declarative only)
- Timing and temporal logic
- Exception handling flows
- Implementation-level constraints

**Tool ecosystem (2024 status):**
- **Drools + SBVR grammar**: Hnatkowska/Gaweda proposed a SBVR-SE language subset that translates to Drools rules via a restricted grammar. Drools is a forward/backward chaining inference engine (Red Hat, open source). DSL extensions make rules closer to natural language.
- **Protégé SBVR plugin**: Converts SBVR vocabularies to OWL ontologies (academic, unmaintained since ~2018)
- **VeTIS** (academic): SBVR validation tool, Java-based
- **KDM Analytics SBVR**: Commercial tool for SBVR → software analytics
- **Automated NL → SBVR**: NLP pipeline (2019 research) extracts SBVR vocabularies and rules from UML use case diagrams

**SBVR adoption reality:** SBVR is widely cited in standards but poorly adopted in practice. The primary barrier is the required mastery of formal concept definitions and the mismatch between SBVR's declarative logic and how practitioners write requirements. Most real-world "SBVR implementations" are simplified subsets with informal semantics.

**SBVR vs. Drools for CodeSpeak:** SBVR provides formal semantics and a standard interchange format; Drools provides execution. The gap is tooling: no production-quality SBVR IDE exists.

### 2.2 ACE — Attempto Controlled English

**Grammar summary:** ACE is a proper subset of English with unambiguous parsing to Discourse Representation Structures (DRS), a first-order logic variant. Key grammar constraints:

- **Determiners**: "a", "every", "each", "all", "no", "some", "at most N", "at least N"
- **Verbs**: must have explicit subjects; passive voice allowed but discouraged (ambiguous agent)
- **Pronouns**: severely restricted — "it" must have an unambiguous antecedent within 2 sentences
- **Negation**: "does not", "is not", but NOT "never" or "hardly ever" (too English-idiomatic)
- **Conjunctions**: "and", "or" allowed; "but", "although", "unless" not in core ACE
- **Relative clauses**: "that", "which" — restrictive only
- **Numbers**: cardinals ("3"), not ordinals ("third") in core

**Tool stack:**
- **APE** (Attempto Parsing Engine): Prolog-based parser, translates ACE → DRS → OWL/SWRL. Available at `http://attempto.ifi.uzh.ch/ape/` (web demo). GitHub: `Attempto/APE`.
- **AceWiki**: Semantic wiki on ACE + APE. Predictive text editor shows only grammatically valid completions. GitHub: `AceWiki/AceWiki`.
- **ACE View**: Protégé plugin for ontology editing in ACE
- **RACE** (Reasoner): ACE → theorem prover; does NOT terminate for infinite models (ACE is not decidable)
- **ACE-in-GF**: ACE formulated in Grammatical Framework (GF), enabling multilingual surface forms of ACE texts

**ACE expressiveness:**
- ACE sentences with only `every`, `a`, `is`, binary verbs → maps to OWL DL fragment (decidable)
- Full ACE → first-order logic (undecidable in general)
- Cannot express: temporal logic, probabilistic statements, meta-level statements about specifications, exception priorities

**Critical limitation for industrial use:** ACE requires writers to learn its restricted grammar. The learning curve (2–4 hours) is manageable, but the restrictions chafe on complex requirements. ACE cannot express "unless", "except when", "in the event that" — common in real requirements.

**Practical ACE example:**
```ace
Every user is a person.
Every person has an account.
If a user logs in then the system authenticates the user.
No user can access a resource that the user does not own.
```
APE translates this to DRS which then maps to OWL axioms verifiable by a reasoner.

### 2.3 Rimay CNL — Full Grammar

**Full structure (from PMC 8550625 / Springer 2021):**

```
requirement = [SCOPE] [CONDITION_STRUCTURE] ACTOR MODAL_VERB SYSTEM_RESPONSE

SCOPE          = "In the context of" NounPhrase
CONDITION_STRUCTURE = WHILE | WHEN | WHERE | IF | TEMPORAL [AND|OR condition]*

WHILE_STRUCTURE   = "While" VP   (system state)
WHEN_STRUCTURE    = "When" VP    (triggering event)
WHERE_STRUCTURE   = "Where" VP   (system feature context)
IF_STRUCTURE      = "If" VP      (precondition)
TEMPORAL_STRUCTURE = "After" | "Before" VP

ACTOR = "the system" | "the user" | DomainNoun
MODAL_VERB = "shall" | "should" | "may" | "must"

SYSTEM_RESPONSE = RESPONSE_BLOCK_ITEMIZED | SYSTEM_RESPONSE_EXPRESSION

RESPONSE_BLOCK_ITEMIZED = ":" newline ("- " ACTION_PHRASE)*
SYSTEM_RESPONSE_EXPRESSION = ACTION_PHRASE [LOGICAL_OP ACTION_PHRASE]*

ACTION_PHRASE = VERB_CODE [frequency] NounPhrase*
VERB_CODE = one of 41 verb codes (32 VerbNet + 9 proposed)
         # VerbNet classes: send-11.1, create-26.4, remove-10.1, update-13.1, ...
         # New: validate, authenticate, authorize, encrypt, log, notify, reject, redirect, store
```

**The 9 new Rimay verb codes** (not in VerbNet, added from financial domain corpus analysis):
`validate`, `authenticate`, `authorize`, `encrypt`, `log`, `notify`, `reject`, `redirect`, `store`

**Evaluation results:**
- 88% representability across 4 unseen SRSs (460 requirements)
- Failure causes:
  - Unsupported verbs (Cause 1, eliminated by saturation): 5%
  - Missing semantic content (domain jargon): 4%
  - Unclear requirements (analyst disagreement): 3%
- 89% precision/recall for smell detection via Paska
- 96% precision / 94% recall for pattern recommendation

**Tool support:** Xtext editor (Eclipse-based) with syntax highlighting, autocomplete, and structural validation. Not available as a standalone LSP or web tool as of 2024.

### 2.4 CNL Comparison for CodeSpeak

| Feature | ACE | SBVR | Rimay |
|---------|-----|------|-------|
| Formal semantics | FOL/DRS | Modal deontic logic | Pattern-based (no formal semantics) |
| Decidability | FOL (no) / OWL subset (yes) | Yes (modal logic) | N/A |
| Expressiveness | High (full FOL) | Medium (business rules) | Medium (functional reqs) |
| Learning curve | High (restrictive English) | Very high (formal concepts) | Low (structured English) |
| Tool maturity | Academic (APE stable, AceWiki stale) | Poor (academic/niche) | Academic (Xtext editor) |
| Closest to prose | Moderate | Low | High |
| NL coverage | ~75% typical reqs | ~60% business rules | 88% functional reqs |
| Industrial adoption | Niche (legal, ontology) | Niche (enterprise rules) | Research (financial domain) |
| Best fit for CodeSpeak | Too restrictive | Too formal | Closest match |

**Recommendation for CodeSpeak:** Rimay's grammar is the closest match — it reads like structured English, has demonstrated 88% coverage of real specs, and has an industrially validated tool (Paska). The main gap: no formal semantics for automated reasoning. The pragmatic path is to use Rimay-style grammar for input, then translate to a simpler formal representation (predicate logic or constraint graph) for consistency checking.

---

## 3. Knowledge Graph Approaches

### 3.1 Entity Graph from NL Specs

**Extraction pipeline:**

```
NL spec → tokenize → NER → dependency parse → relation extraction → graph construction → entity resolution
```

**Step 1: NER for spec entities**

Standard spaCy `en_core_web_trf` identifies `ORG`, `PERSON`, `GPE`, `PRODUCT`. For specs, you need domain-specific entity types. Use spaCy's `EntityRuler` with patterns:

```python
import spacy
from spacy.pipeline import EntityRuler

nlp = spacy.load("en_core_web_sm")
ruler = nlp.add_pipe("entity_ruler", before="ner")
ruler.add_patterns([
    {"label": "SYSTEM", "pattern": [{"LOWER": "the"}, {"LOWER": "system"}]},
    {"label": "ACTOR", "pattern": [{"LOWER": "the"}, {"LOWER": "user"}]},
    {"label": "RESOURCE", "pattern": [{"DEP": "dobj"}]},  # objects of actions
    {"label": "ACTION", "pattern": [{"POS": "VERB"}, {"DEP": "ROOT"}]},
])
```

**Step 2: Relation extraction via dependency parsing**

Subject-verb-object triples from dependency parse (universal dependencies):
```python
def extract_svo(doc):
    triples = []
    for token in doc:
        if token.dep_ == "ROOT" and token.pos_ == "VERB":
            subj = [t for t in token.lefts if t.dep_ in ("nsubj", "nsubjpass")]
            obj = [t for t in token.rights if t.dep_ in ("dobj", "iobj", "pobj")]
            if subj and obj:
                triples.append((subj[0].lemma_, token.lemma_, obj[0].lemma_))
    return triples
```

**Step 3: Graph construction**

```python
import networkx as nx

def build_spec_graph(requirements: list[str]) -> nx.DiGraph:
    G = nx.DiGraph()
    for req in requirements:
        doc = nlp(req)
        for subj, verb, obj in extract_svo(doc):
            G.add_node(subj, type="entity")
            G.add_node(obj, type="entity")
            G.add_edge(subj, obj, relation=verb, source=req)
    return G
```

### 3.2 Neo4j vs In-Memory Graph

| Factor | Neo4j | NetworkX (in-memory) |
|--------|-------|---------------------|
| Scale | Billions of nodes | Millions of nodes (RAM-bound) |
| Query language | Cypher (declarative) | Python API |
| Startup cost | Server process, Java heap | Instant, embedded |
| Consistency checks | ACID transactions, OGM | Manual |
| Graph algorithms | GDS plugin (built-in) | `networkx.algorithms` |
| Embedding similarity | Vector index (v5.x) | `numpy` + `faiss` |
| Best for | Production, multi-user, persistence | Prototyping, CI pipelines, small corpora |

**For a spec linter CI pipeline:** NetworkX in-memory is the right choice. A 500-requirement spec generates ~2,000 nodes and ~5,000 edges — trivial for in-memory processing.

**Neo4j LLM Graph Builder (2024):** Neo4j Labs released `llm-graph-builder` which uses `llm-graph-transformer` (OpenAI/Gemini/Claude) to extract entity graphs from unstructured documents. Schema-guided extraction: you define node labels and relationship types, and the LLM respects them. Entity resolution merges duplicate nodes via configurable merge strategies.

**Cypher query for cross-module consistency:**
```cypher
// Find entities defined in module A but referenced differently in module B
MATCH (a:Entity {module: "auth"})-[:DEFINES]->(e:Term)
MATCH (b:Entity {module: "payments"})-[:REFERENCES]->(e2:Term)
WHERE e.canonical_name <> e2.canonical_name
  AND e.lemma = e2.lemma
RETURN a.module, e.canonical_name, b.module, e2.canonical_name
```

### 3.3 Graph-Based Contradiction Detection

**Algorithms:**

**1. Direct contradiction via relation polarity:**
Two nodes A → B (relation: "allows") and A → B (relation: "prohibits") on the same path = contradiction. Detection: O(E) where E is number of edges.

```python
def detect_polarity_contradictions(G: nx.DiGraph) -> list:
    issues = []
    for u, v, data in G.edges(data=True):
        other_edges = [
            d for u2, v2, d in G.edges(data=True)
            if u2 == u and v2 == v and d != data
        ]
        for other in other_edges:
            if is_opposite(data['relation'], other['relation']):
                issues.append((u, v, data['source'], other['source']))
    return issues

OPPOSITES = {
    'allow': 'prohibit', 'enable': 'disable',
    'require': 'forbid', 'include': 'exclude'
}
def is_opposite(r1, r2): return OPPOSITES.get(r1) == r2 or OPPOSITES.get(r2) == r1
```

**2. Cycle detection for circular constraints:**
A requires B which requires A — logical inconsistency. Use `nx.find_cycle()`. Complexity: O(V + E).

**3. Reachability contradiction:**
Entity X is reachable via "must" chain from both "allowed" and "forbidden" states. Uses BFS with label tracking. O(V + E).

**4. OWL anti-pattern detection (GLaMoR approach):**
GLaMoR (Graph Language Models for Reasoning, arxiv 2504.19023) converts OWL ontologies to graphs, trains a T5-based graph language model as a classifier. Results: **95.13% accuracy, 96.10% precision**, 20× faster than classical HermiT reasoner (6h training vs 122h+ reasoning).

Key insight: consistency checking as graph **classification** (consistent/inconsistent) rather than as symbolic reasoning. The 14 OWL anti-patterns used as inconsistency templates are:
- Disjointness violations (A subclassOf B, A disjointWith B)
- Cyclic subsumption (A subclassOf B subclassOf A)
- Universal-existential conflicts
- Cardinality violations (min > max)
- Domain-range violations

Scalability: modularization via OAPT reduces class count by 29.82% and properties by 83.33%, enabling processing of biomedical ontologies with thousands of axioms.

**For spec graphs:** The GLaMoR approach translates well — represent spec entities as graph nodes, constraints as typed edges, then train a classifier on examples of consistent vs. inconsistent spec modules. Key requirement: labeled training data (consistent/inconsistent spec pairs).

### 3.4 Embedding-Based Entity Deduplication

When building an entity graph from multiple spec modules, the same concept appears under different names: "auth token" / "authentication token" / "bearer token" / "JWT". Deduplication prevents false "not defined" errors.

**Embedding pipeline:**
```python
from sentence_transformers import SentenceTransformer
import numpy as np
from sklearn.cluster import AgglomerativeClustering

model = SentenceTransformer('all-MiniLM-L6-v2')  # fast, good quality

def deduplicate_entities(entity_names: list[str], threshold: float = 0.85) -> dict:
    """Returns mapping: entity_name → canonical_name"""
    embeddings = model.encode(entity_names)

    # Cosine similarity matrix
    norms = np.linalg.norm(embeddings, axis=1, keepdims=True)
    similarity = (embeddings / norms) @ (embeddings / norms).T
    distance = 1 - similarity

    clustering = AgglomerativeClustering(
        metric='precomputed',
        linkage='complete',
        distance_threshold=1 - threshold,
        n_clusters=None
    )
    labels = clustering.fit_predict(distance)

    # For each cluster, pick the shortest name as canonical (or the most frequent)
    clusters = {}
    for name, label in zip(entity_names, labels):
        clusters.setdefault(label, []).append(name)

    mapping = {}
    for label, names in clusters.items():
        canonical = min(names, key=len)  # or use frequency
        for name in names:
            mapping[name] = canonical

    return mapping
```

**Threshold guidance:**
- `0.95`: very strict, only catches obvious duplicates ("auth token" / "auth_token")
- `0.85`: catches synonyms ("authentication token" / "auth token")
- `0.75`: catches related concepts — may over-merge ("token" / "session token")
- `0.65`: aggressive — causes false merges in rich domains

NVIDIA NeMo SemDedup uses `eps=0.01` (1% distance = 99% similarity) for training data deduplication, removing up to 50% of web-scale data with minimal performance loss. For spec entities, 85% similarity (threshold=0.85) is a good starting point.

---

## 4. LLM-Based Consistency Checking

### 4.1 Direct LLM Consistency Checking

**What works:**

From ALICE-style research and the Frontiers systematic review (fcomp.2025.1519437):
- GPT-4o and Claude 3.5 Sonnet achieve F1 79–94% for identifying unfulfilled/contradicting requirements when given structured prompts
- GPT-3.5-turbo: ~50% F1 (not viable for production)
- Chain-of-thought + few-shot significantly improves results over zero-shot
- Fewer requirements per prompt → better accuracy (split into chunks of 10–20 requirements)
- Structured concise spec language outperforms verbose prose

**Effective prompt pattern for consistency checking:**

```
System: You are a requirements consistency checker. Given a list of requirements, identify all contradictions — pairs or groups of requirements that cannot all be simultaneously satisfied. For each contradiction: (1) identify the requirement IDs, (2) explain why they contradict, (3) suggest a resolution.

User:
Requirements:
[R1] The system SHALL encrypt all data at rest using AES-256.
[R2] The system SHALL store user preferences in plaintext for performance.
[R3] The system SHALL comply with GDPR data minimization principles.
[R4] The system SHALL log all user actions with full payloads for audit.

Task: Find all contradictions. For each, output:
- Contradiction: R? vs R?
- Reason: (1 sentence)
- Resolution: (1 sentence)
```

**Few-shot example (improves recall significantly):**
Add 2–3 labeled contradiction examples before the task. Key: examples should demonstrate the types of contradictions common in your domain (e.g., security vs performance, privacy vs auditability).

**Multi-pass verification pattern:**
1. **Pass 1 (LLM):** Find candidate contradictions → JSON list `[{req_a, req_b, reason, confidence}]`
2. **Pass 2 (formal):** For high-confidence candidates, attempt to formalize and verify with Z3/SMT solver
3. **Pass 3 (LLM):** Explain verified contradictions in plain language and suggest fixes

```python
import anthropic
import json

client = anthropic.Anthropic()

def check_consistency_llm(requirements: list[dict]) -> list[dict]:
    req_text = "\n".join(f"[{r['id']}] {r['text']}" for r in requirements)

    response = client.messages.create(
        model="claude-sonnet-4-6",
        max_tokens=2000,
        messages=[{
            "role": "user",
            "content": f"""Check these requirements for contradictions. Output JSON only.

Requirements:
{req_text}

Output format:
[{{"req_a": "R1", "req_b": "R2", "reason": "...", "confidence": 0.9}}]

JSON:"""
        }]
    )
    return json.loads(response.content[0].text)
```

### 4.2 Cost Analysis: Checking a 100-Module Spec

**Assumptions:**
- 100 modules × 20 requirements each = 2,000 requirements total
- Average requirement: 25 tokens
- Checking one module of 20 reqs: ~500 input tokens + 300 output tokens = 800 tokens
- Cross-module check (pairs of modules): C(100,2) = 4,950 pairs × ~1,000 tokens = 4.95M tokens

**Pricing (March 2026 approximate):**
- GPT-4o: $5/M input, $15/M output
- Claude Sonnet 4.6: $3/M input, $15/M output
- GPT-4o-mini: $0.15/M input, $0.60/M output
- Gemini 1.5 Flash: $0.075/M input, $0.30/M output

**Cost breakdown for full 100-module spec check:**

| Scope | Tokens | GPT-4o | Claude Sonnet | GPT-4o-mini |
|-------|--------|--------|---------------|-------------|
| Within-module only (100 checks) | 80K | $0.40 | $0.24 | $0.012 |
| All cross-module pairs (4,950) | 4.95M | $24.75 | $14.85 | $0.74 |
| Full system (all + cross) | 5.03M | $25.15 | $15.09 | $0.75 |

**Practical strategy:**
1. Run within-module checks with Claude Sonnet/GPT-4o for accuracy (~$0.24–0.40)
2. Run cross-module checks with GPT-4o-mini or Gemini Flash (~$0.74–1.50)
3. Escalate only flagged pairs to GPT-4o for final verdict
4. Total cost for 100-module spec: ~$2–5

**Token optimization:** Compress requirements before sending — strip boilerplate, keep core predicate. "The system SHALL ensure that when a user initiates a logout, the user's session token is invalidated within 30 seconds" → "logout → invalidate session_token within 30s". Reduces token count by 40–60%.

### 4.3 Accuracy Comparison: LLM-only vs Hybrid vs Formal-only

| Approach | Precision | Recall | Setup cost | Latency | False positive rate |
|----------|-----------|--------|------------|---------|---------------------|
| Formal-only (NLI/DRS) | ~85% | ~45% | Medium | Fast | ~15% |
| LLM-only (GPT-4o, zero-shot) | ~80% | ~55% | Low | Slow | ~20% |
| LLM-only (few-shot, CoT) | ~87% | ~65% | Low | Slow | ~13% |
| Hybrid (ALICE-style) | ~99% | ~60% | High | Medium | ~1% |
| Full formal (ACE+reasoner) | ~100% | ~35% | Very high | Variable | ~0% |

**Key insight:** Hybrid wins on precision. Recall ceiling (~60%) is the current frontier — no approach finds all contradictions because some require domain knowledge not present in the spec text.

**The ALICE recall gap:** ALICE gets 99% precision but only 60% recall. The 40% it misses are "soft" contradictions requiring world knowledge ("AES-256 at rest" contradicts "plaintext for performance" only if you know encryption has CPU overhead).

---

## 5. Gap Detection Deep Dive

### 5.1 Taxonomy of Spec Gaps

Gap types, ordered by frequency in real industrial specs (based on research and INCOSE v4 analysis):

**Type 1: Missing Behaviors (most common ~40%)**
- Happy path exists but error path absent
- "The system SHALL authenticate the user" — no spec for authentication failure
- "The system SHALL process the payment" — no spec for payment timeout, decline, partial failure

**Type 2: Missing Error Cases (~25%)**
- HTTP 4xx/5xx responses not specified
- Timeout conditions unspecified
- Concurrent request handling not addressed
- Resource exhaustion behavior absent

**Type 3: Missing Constraints (~20%)**
- "The user SHALL be able to upload files" — no size limit, type restriction, or rate limit
- "The system SHALL store data" — no retention policy, no deletion spec
- "The API SHALL return results" — no pagination spec, no maximum result count

**Type 4: Missing Edge Cases (~10%)**
- Empty collection behavior ("return results" — what if empty?)
- Null/optional field handling
- Maximum cardinality not tested ("up to" — what happens at the boundary?)

**Type 5: Missing Non-Functional Coverage (~5%)**
- Performance thresholds absent for specified operations
- Concurrency behavior absent
- Security spec exists at perimeter but absent at inner layers

### 5.2 Automated Enumeration of What's NOT Specified

**Pattern-based gap inference:**

For every requirement of form `ACTOR SHALL ACTION(resource)`, check if the following derived requirements exist:

```python
GAP_INFERENCE_RULES = [
    # For each action, the corresponding failure case should exist
    ("create {R}", "should have: {ACTOR} SHALL handle {R} creation failure"),
    ("delete {R}", "should have: {ACTOR} SHALL handle {R} not found"),
    ("update {R}", "should have: {ACTOR} SHALL handle concurrent update conflict"),
    ("retrieve {R}", "should have: {ACTOR} SHALL handle {R} not found"),
    ("authenticate {U}", "should have: {ACTOR} SHALL handle authentication failure"),
    ("send {M} to {R}", "should have: {ACTOR} SHALL handle {R} unavailable"),
]

def infer_gaps(requirements: list[dict]) -> list[str]:
    req_texts = set(r['text'].lower() for r in requirements)
    gaps = []

    for req in requirements:
        for pattern, derived in GAP_INFERENCE_RULES:
            if matches(req['text'], pattern):
                expected = expand_template(derived, req)
                if not any(similar(expected, existing) for existing in req_texts):
                    gaps.append(f"Missing: {expected} (derived from {req['id']})")
    return gaps
```

**LLM-based gap enumeration:**

```
System: You are a requirements completeness checker. Given a list of requirements, identify the missing requirements — behaviors, constraints, and error cases that should be specified but are absent. Be specific: for each gap, name the exact scenario that is unspecified.

User:
[R1] The user SHALL be able to upload a file.
[R2] The system SHALL store the file in object storage.
[R3] The system SHALL return the file's URL after upload.

Missing requirements (list at least 5):
```

This prompt pattern reliably elicits: max file size, allowed MIME types, upload failure behavior, storage failure behavior, duplicate file handling, concurrent upload behavior.

**LLM-aided verification gap detection (TechRxiv 2024):** Research on LLM-aided verification gap detection for UVM testbenches (hardware verification) shows that LLMs can identify missing checkers by comparing spec behaviors against test coverage. The approach: `spec_behaviors = LLM.extract(spec)`, `covered = LLM.identify(testbench)`, `gaps = spec_behaviors - covered`. Directly applicable to software requirements.

### 5.3 Coverage Metrics for Specs

No universally adopted "spec coverage" metric exists (analogous to code coverage). Proposed metrics from research:

**Requirement Coverage (RC):**
```
RC = |requirements_with_tests| / |total_requirements|
```
Measures horizontal coverage (which requirements are tested) but not vertical coverage (how thoroughly each requirement is tested).

**Scenario Coverage (SC):**
For each requirement, enumerate expected scenarios (happy path + N error paths). SC = tested scenarios / total expected scenarios.

**State Coverage (from arxiv 2510.03071):**
"State field coverage" — measures what fraction of an object's state is checked by an oracle during test execution. Directly applicable to spec coverage: for each entity in the spec, what fraction of its state transitions are specified?

**Behavior Coverage (BC) — proposed:**
```
BC = |specified_behaviors| / |specified_behaviors + inferred_missing_behaviors|
```
Where `inferred_missing_behaviors` comes from the gap inference rules above. If a spec has 50 requirements and gap analysis infers 25 missing ones, BC = 50/75 = 67%.

### 5.4 Interactive Gap-Filling UX

**MCQ (Multiple Choice Question) pattern — from AmbiSQL:**
For each detected gap, generate: question + 2–4 options + "other / skip".

```
Gap detected: No error handling specified for authentication failure.

How should the system handle a failed login attempt?
A) Return HTTP 401 with error message
B) Return HTTP 403 with redirect to error page
C) Lock account after N failed attempts, return HTTP 423
D) All of the above (specify each case separately)
E) Skip — document later
```

**Decision tree pattern:**
For complex gaps, generate a conditional question tree:
- "Does the system need to limit upload size?" → Yes/No
  - Yes: "What is the maximum file size? (MB)"
  - Yes: "Should this be configurable per user role?" → Yes/No

**Suggestion-first pattern (reduces cognitive load):**
Show LLM-generated candidate requirements for user approval/rejection:
```
Suggested requirement (based on gap analysis):
  [DRAFT] The system SHALL reject file uploads exceeding 50MB with HTTP 413.
  Accept | Edit | Reject
```

**RAG for gap filling:** Only 6% of LLM-aided RE studies use RAG (Frontiers 2025). But RAG is particularly effective for gap filling: retrieve similar requirements from a corpus of high-quality specs (e.g., IEEE 830 examples, INCOSE GfWR examples) and use them as few-shot examples when generating gap suggestions. This significantly improves suggestion quality.

---

## 6. Cross-Module Consistency

### 6.1 Microservice Spec Interfaces

When a system is composed of multiple spec modules (microservices, components, bounded contexts), consistency checking must span modules.

**Interface contract model:**

```
ModuleA_spec.md: "The auth service SHALL return a token upon successful authentication."
ModuleB_spec.md: "The payments service SHALL accept a bearer token for authorization."
```

The implicit contract: ModuleA produces "token", ModuleB consumes "token" as "bearer token". The spec linter must:
1. Extract entity exports and imports from each module
2. Match exports to imports using entity deduplication
3. Flag mismatches (type, format, cardinality)

**Entity export/import model:**
```python
class ModuleInterface:
    module_id: str
    exports: list[Entity]    # entities produced by this module
    imports: list[Entity]    # entities consumed from other modules
    constraints: list[Constraint]  # constraints on interface entities

class Entity:
    name: str
    canonical_name: str     # after deduplication
    type_constraints: list[str]  # e.g., ["JWT", "expires_in < 3600"]
    source_requirement: str

def check_interface_compatibility(a: ModuleInterface, b: ModuleInterface) -> list[Issue]:
    issues = []
    for imp in b.imports:
        matching_export = find_matching_export(imp, a.exports)
        if not matching_export:
            issues.append(Issue(type="missing_export", entity=imp.name,
                               consumer=b.module_id, provider=a.module_id))
        else:
            # Check type constraint compatibility
            for constraint in imp.type_constraints:
                if not satisfies(matching_export, constraint):
                    issues.append(Issue(type="type_mismatch", ...))
    return issues
```

### 6.2 Breaking Change Detection for Spec Modules

**oasdiff (2024):** The most mature breaking change detector for OpenAPI specs. 300+ breaking change rules, organized by:
- Removed endpoints/methods
- Required fields added to requests
- Response schema narrowed
- Status codes removed
- Auth requirements changed
- Header changes

**CI integration:**
```bash
# Check if spec changes are breaking
oasdiff breaking old-spec.yaml new-spec.yaml --fail-on ERR
# Returns exit code 1 if breaking changes found
```

**Extending to non-OpenAPI NL specs:**

The conceptual model from oasdiff applies: a spec defines a contract (what the module produces/accepts). Any change that invalidates a consumer's assumptions is "breaking".

For NL spec modules, breaking changes include:
- Removing a requirement that a downstream module depends on
- Changing a required entity's properties (type, cardinality, format)
- Changing a precondition (making it stricter)
- Removing a guarantee (making it weaker)
- Changing error behavior (downstream may depend on specific error codes)

**Transitive impact analysis:**

```python
def compute_impact(changed_module: str, spec_graph: nx.DiGraph) -> set[str]:
    """Find all modules transitively affected by changes to changed_module."""
    affected = set()
    queue = [changed_module]
    while queue:
        m = queue.pop()
        for consumer in spec_graph.successors(m):
            if consumer not in affected:
                affected.add(consumer)
                queue.append(consumer)
    return affected
```

This is a standard BFS on the module dependency graph — O(V + E). For 100 modules, this runs in microseconds.

### 6.3 Semantic Versioning for Spec Modules

Applying SemVer to spec modules:

- **MAJOR** (breaking): remove/narrow guaranteed behavior, add required input constraint
- **MINOR** (additive): add new optional behavior, add new optional output field
- **PATCH** (fix): clarify ambiguous wording, fix typo, improve constraint precision

**Automated compatibility check (A v2 + B v1):**

```python
def check_version_compatibility(
    module_a: SpecModule, version_a: str,
    module_b: SpecModule, version_b: str
) -> CompatibilityResult:

    # Check if A's v2 interface is backward compatible with what B v1 expects
    b_v1_assumptions = module_b.extract_assumptions_about(module_a.id)
    a_v2_guarantees = module_a.extract_guarantees()

    violations = [
        assumption for assumption in b_v1_assumptions
        if not is_satisfied_by(assumption, a_v2_guarantees)
    ]

    return CompatibilityResult(
        compatible=len(violations) == 0,
        violations=violations,
        summary=f"{'Compatible' if not violations else 'Breaking'}: "
                f"A{version_a} + B{version_b}"
    )
```

**Consumer-driven contract testing (Pact model) for NL specs:**

PactFlow's bi-directional contract testing compares consumer Pact contracts against provider OAS specs. The same model applies to NL specs: the consumer spec defines what it expects from the provider, the provider spec defines what it guarantees, and a compatibility matrix verifies the match.

---

## 7. Real-World Requirements Quality Data

### 7.1 Defect Distribution in Real Specs

**Published empirical data:**

From the INCOSE GfWR v4 (2023) and Paska industrial evaluation (FSE 2024, 2,725 annotated requirements from 13 systems in finance):

| Defect type | Frequency in industrial reqs | Detection difficulty |
|-------------|------------------------------|----------------------|
| Vague terms | 35–40% | Easy (pattern matching) |
| Incomplete behavior | 25–30% | Medium (inference required) |
| Passive voice / ambiguous agent | 15–20% | Easy (POS tagging) |
| Inconsistency / contradiction | 10–15% | Hard (cross-req analysis) |
| Non-atomic (multiple actions) | 10–12% | Medium (sentence structure) |
| Incorrect ordering (condition after response) | 5–8% | Easy (structure check) |
| Untestable (no success criteria) | 15–20% | Hard (completeness reasoning) |

**Ambiguity vs incompleteness:** From arxiv 2503.17936 empirical study across 6 benchmark datasets:
- MedDialog: 92% incomplete, 8% ambiguous (domain with high implicit knowledge)
- MultiWOZ: 21% incomplete, 75% ambiguous (task-oriented with underspecified goals)
- ShARC: 28% incomplete, 61% ambiguous (conditional rule-following)
- General finding: **ambiguity dominates in conversational/task specs; incompleteness dominates in domain-knowledge-heavy specs**

**INCOSE GfWR study on INCOSE rules in industry (academia, ~2016, still cited):** 56% of software defects originate in requirements and design phases. 48% of those arise in requirements analysis. This 56% × 48% ≈ 27% of all defects trace to requirements problems.

### 7.2 The 1:10:100 Rule — Current Status

Boehm's cost-of-fixing-defects-by-phase rule (requirements fix costs 1× → design 10× → production 100×):

**Current evidence:**
- Original Boehm data (1975): student projects, small N, noted as "notional" not measured
- NIST 2003 study: software defects cost US economy $59.5B/year (does not validate multipliers)
- NASA study (ntrs.nasa.gov/20100036670): exponential cost increase confirmed but multipliers vary by project type
- Modern critique (Morendil/GitHub gist): "many attempts to measure this over a long period of time with very different results"

**What's better supported:** The direction (later is more expensive) is validated. The exact multiplier (10×, 100×) is not reliably measured. Modern CI/CD with automated testing reduces the gap because production defects are caught faster.

**Practical implication for spec quality tools:** The business case exists (early fixing is cheaper) but don't cite "100×" without qualification. A more defensible claim: "defects found by automated spec analysis before implementation save at minimum 3–10× the cost of fixing them in code review."

### 7.3 Industries with Best Requirements Practices

**Aerospace (DO-178C, AS9100):**
- Target: < 0.1–0.5 defects per KLOC
- Requirements must be formally traceable to test cases
- MBSE (Model-Based Systems Engineering) with SysML for critical components
- Change control: every requirement change triggers impact analysis
- Gap from CodeSpeak: extremely process-heavy, unsuitable for fast-moving software

**Automotive (ISO 26262, ASPICE SWE.1):**
- ASPICE level 3 requires: requirements structured, traceable, complete
- Every SW requirement linked to system requirement and test case
- EAST-ADL for architectural requirements (where Rimay CNL was originally applied)
- Automated consistency checking is expected in level 4+ processes
- Gap: long change cycles, safety-critical conservatism, not directly applicable to web-scale services

**Finance (Paska's domain):**
- Rimay CNL was validated on 13 financial systems with 2,725 requirements
- 89% smell detection precision in financial specs — suggests financial specs have distinct patterns
- Regulatory requirements (Basel III, PCI-DSS, GDPR) drive formalization
- Best practice: requirements in a controlled language, reviewed by both business and legal

**Healthcare (FDA 21 CFR Part 11):**
- Stricter than finance on traceability
- Every requirement linked to validation test with documented evidence

**What high-quality industries do differently:**
1. **Structured language**: requirements follow templates (shall/actor/action/object/condition)
2. **Glossary enforcement**: canonical terms maintained, all requirements reviewed against glossary
3. **Traceability matrix**: every requirement links to test case and design artifact
4. **Automated quality gates**: tools like DOORS, Polarion, or Reqtify check quality before review
5. **Domain-specific CNL**: automotive uses EAST-ADL grammar, aerospace uses SSS/SRS templates
6. **Two-person review**: requirements reviewed by author + domain expert + QA engineer
7. **Change impact analysis**: no requirement changes without automated impact report

---

## 8. Practical Synthesis — What to Build for CodeSpeak

### 8.1 Architecture Decision: The Lean Path

Based on the research, the pragmatic lean-path architecture for a CodeSpeak spec linter:

```
Spec input (Markdown/structured text)
    ↓
┌───────────────────────────────────────────────┐
│  Layer 1: Structural Lint (Vale, fast)         │
│  - Vague verbs, passive voice, canonical terms  │
│  - ~100ms, zero false negatives on patterns    │
└───────────────────────────────────────────────┘
    ↓ (if issues < threshold)
┌───────────────────────────────────────────────┐
│  Layer 2: Semantic Graph (spaCy + NetworkX)    │
│  - SVO extraction, entity deduplication        │
│  - Polarity contradiction, cycle detection     │
│  - ~500ms for 200 requirements                 │
└───────────────────────────────────────────────┘
    ↓ (candidates from layer 2)
┌───────────────────────────────────────────────┐
│  Layer 3: NLI Cross-Check (DeBERTa)            │
│  - Pairwise contradiction on graph candidates  │
│  - Pre-filtered by cosine similarity (>0.3)    │
│  - ~1-2s on CPU, ~200ms on GPU                 │
└───────────────────────────────────────────────┘
    ↓ (if high-value spec / release gate)
┌───────────────────────────────────────────────┐
│  Layer 4: LLM Gap Detection + Explanation      │
│  - Claude Sonnet: gap inference, MCQ generation│
│  - ~$0.24 for 100-module spec (within modules) │
│  - Optional: cross-module at ~$2-5 total       │
└───────────────────────────────────────────────┘
    ↓
┌───────────────────────────────────────────────┐
│  Reporter: JSON / LSP diagnostics / SARIF      │
│  - Severity: error / warning / info            │
│  - Source requirement IDs                      │
│  - Suggested fixes                             │
└───────────────────────────────────────────────┘
```

### 8.2 CNL Choice for CodeSpeak

**Recommendation: Rimay-inspired (not full Rimay)**

Use Rimay's structure (actor / modal verb / condition / action) as the preferred input format but without enforcing the full grammar. Validate structure with regex + spaCy, not a full CNL parser. This gives 80% of the benefit at 10% of the implementation cost.

Core template:
```
[When <condition>,] [the <actor>] <modal_verb> <verb_phrase> [<object>] [within <constraint>].
```

Validate:
1. Modal verb present (shall/should/may/must)
2. Actor explicit (not implied)
3. Verb code from allowed list (no "process", "handle", "manage")
4. Condition clause syntactically complete (if used)

### 8.3 Cross-Module Check for a Spec Network

```python
# spec_network.py
from dataclasses import dataclass
import networkx as nx

@dataclass
class SpecModule:
    id: str
    path: str
    entities: dict[str, str]  # name → canonical_name
    exports: list[str]  # canonical entity names this module guarantees
    imports: list[str]  # canonical entity names this module consumes

def build_module_graph(modules: list[SpecModule]) -> nx.DiGraph:
    G = nx.DiGraph()
    for m in modules:
        G.add_node(m.id, module=m)

    # Add edges: consumer → provider (consumer needs provider's exports)
    for consumer in modules:
        for imp in consumer.imports:
            for provider in modules:
                if imp in provider.exports:
                    G.add_edge(consumer.id, provider.id,
                              via=imp, type="depends_on")
    return G

def check_cross_module(modules: list[SpecModule]) -> list[dict]:
    issues = []
    G = build_module_graph(modules)

    # 1. Unresolved imports
    all_exports = {exp for m in modules for exp in m.exports}
    for m in modules:
        for imp in m.imports:
            if imp not in all_exports:
                issues.append({
                    "type": "unresolved_import",
                    "module": m.id,
                    "entity": imp,
                    "severity": "error"
                })

    # 2. Circular dependencies
    try:
        cycle = nx.find_cycle(G)
        issues.append({
            "type": "circular_dependency",
            "cycle": [e[0] for e in cycle],
            "severity": "warning"
        })
    except nx.NetworkXNoCycle:
        pass

    # 3. Transitively affected modules for each change
    # (Use BFS from changed node — O(V+E))

    return issues
```

---

## Sources

All primary sources used in this document:

- Vale CLI documentation: https://vale.sh/docs/checks/existence, https://vale.sh/docs/checks/substitution
- Vale GitHub: https://github.com/errata-ai/vale
- cross-encoder/nli-deberta-v3-base model card: https://huggingface.co/cross-encoder/nli-deberta-v3-base
- GLaMoR paper: https://arxiv.org/html/2504.19023v1
- Dealing with Inconsistency KG Survey: https://arxiv.org/abs/2502.19023
- Rimay CNL paper (PMC): https://pmc.ncbi.nlm.nih.gov/articles/PMC8550625/
- Rimay/Paska (FSE 2024): https://2024.esec-fse.org/details/fse-2024-journal-first/5/
- Paska paper: https://arxiv.org/html/2305.07097
- Incompleteness/Ambiguity empirical study: https://arxiv.org/abs/2503.17936
- SBVR OMG spec: https://www.omg.org/spec/SBVR/1.5/About-SBVR
- ACE Wikipedia: https://en.wikipedia.org/wiki/Attempto_Controlled_English
- APE GitHub: https://github.com/Attempto/APE
- AceWiki GitHub: https://github.com/AceWiki/AceWiki
- ACE-in-GF GitHub: https://github.com/Attempto/ACE-in-GF
- Neo4j LLM Graph Builder: https://neo4j.com/labs/genai-ecosystem/llm-graph-builder/
- NVIDIA NeMo SemDedup: https://docs.nvidia.com/nemo-framework/user-guide/24.09/datacuration/semdedup.html
- Frontiers LLM in RE systematic review: https://www.frontiersin.org/journals/computer-science/articles/10.3389/fcomp.2025.1519437/full
- LLM-aided verification gap detection: https://www.techrxiv.org/users/1024156/articles/1383980-llm-aided-verification-gap-detection-a-methodology-for-identifying-missing-checkers-in-uvm-testbenches
- oasdiff breaking change detection: https://www.oasdiff.com/
- PactFlow bi-directional contract testing: https://pactflow.io/bi-directional-contract-testing/
- PactFlow OAS contracts: https://docs.pactflow.io/docs/bi-directional-contract-testing/contracts/oas/
- INCOSE GfWR v4 summary: https://www.incose.org/docs/default-source/working-groups/requirements-wg/guidetowritingrequirements/incose_rwg_gtwr_v4_summary_sheet.pdf
- Boehm defect cost critique: https://gist.github.com/Morendil/ebfa32d10528af04e2ccb8995e3cb4a7
- NASA error cost escalation: https://ntrs.nasa.gov/api/citations/20100036670/downloads/20100036670.pdf
- SBVR + Drools: http://blog.athico.com/2007/03/standards-based-approach-to-natural.html
- NLP4RE tools: https://github.com/JulianFrattini/nlp4re-tools
- Requirements traceability survey: https://en.wikipedia.org/wiki/Requirements_traceability
- LegalLens 2024 DeBERTa NLI: https://arxiv.org/html/2410.22977v1
- Moritz Laurer DeBERTa zero-shot: https://huggingface.co/MoritzLaurer/deberta-v3-large-zeroshot-v1.1-all-33
- spaCy knowledge graph: https://memgraph.com/blog/extract-entities-build-knowledge-graph-memgraph-spacy
- INCOSE GfWR empirical: https://www.academia.edu/90608972/Applying_INCOSE_Rules_for_writing_high-quality_requirements_in_Industry
- Automotive requirements ASPICE SWE.1: https://www.ul.com/sis/resources/process-swe-1
