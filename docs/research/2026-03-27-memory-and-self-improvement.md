# Memory & Self-Improvement: Deep Research Findings

## Part 1: Memory Architectures

### State of the Art (2025-2026)

| System | Architecture | Storage | Retrieval | Benchmark |
|--------|-------------|---------|-----------|-----------|
| **Mem0** | Extract facts + ADD/UPDATE/DELETE vs existing | Vector DB + graph | Hybrid (vector + graph) | +26% vs OpenAI memory on LoCoMo |
| **Zep/Graphiti** | Temporal knowledge graph (3 layers) | Neo4j graph | BM25 + vector + graph traversal | 94.8% on DMR |
| **Letta/MemGPT** | LLM as OS: core (RAM) / recall (episodic) / archival (disk) | Vector + filesystem | Agent-driven iterative search | 74% on LoCoMo with just grep |
| **Cognee** | Knowledge engine -> KG triplets | Graph + embeddings | Graph-aware embeddings | N/A |
| **Honcho** | Entity-centric user modeling, Theory of Mind | Cloud backend | Dialectic query (LLM reasoning) | N/A |
| **Engram (ours)** | 3-layer: L1 handbook / L2 MCP / L3 auto-memory | Flat files + MCP | Keyword search + topic keys | N/A |

### Key Insight: Letta Benchmark

**"Is a Filesystem All You Need?"** - Letta's benchmark showed that a simple filesystem (grep + open + iterative reformulation) scored 74% on LoCoMo vs 68.5% for Mem0's vector DB. The agent's ability to reformulate queries matters more than the storage backend.

This validates our flat-file approach. The question isn't "which DB?" but "how smart is the retrieval agent?"

### Memory Taxonomy (3 dimensions)

**By function:**
- **Episodic** - what happened (task receipts, session logs)
- **Semantic** - what is true (project facts, conventions)
- **Procedural** - how to do things (skills, workflows)
- **Working** - active context (current task state)

**Applied to specpunk:**
| Memory Type | Current Implementation | Proposed |
|-------------|----------------------|----------|
| Episodic | audit.jsonl, digest.jsonl | receipts/index.jsonl (structured, versioned) |
| Semantic | Engram L2, bank/ handbook | Engram stays (SSoT for facts) |
| Procedural | CC skills, punk templates | Skills as markdown (already have) |
| Working | Task JSON + session context.json | Frozen snapshot at task start |

### Consolidation & Forgetting

**Sleep-Inspired Consolidation** (arxiv:2603.14517):
- Stale entries in memory actively HARM performance (proactive interference)
- SleepGate: conflict detection + forgetting gate + consolidation module
- Result: 99.5% accuracy vs 10% baseline at depth=5

**Our Auto Dream** already does this:
- Runs every 24h (>=5 sessions)
- Deduplicates, removes stale entries
- Converts relative dates to absolute
- Keeps MEMORY.md <= 200 lines

**For specpunk sessions:**
- TTL-based eviction (entry.ttl_tasks counts down per task)
- Typed entries (success/failure/surprise/cost_overrun) - failures weighted higher
- Negative signal forced: receipt validator rejects receipts without at least one session entry

### Security

**MINJA attack** (NeurIPS 2025): 95% injection success rate against production agents via query-only interaction. OWASP ASI06 top agentic risk 2026.

**Hermes defense**: 10 regex patterns for prompt injection + invisible Unicode scan before memory injection.

**For specpunk:** Session context.json must be scanned before injection. Receipt `summary` field is agent-written and untrusted.

---

## Part 2: Self-Improvement Mechanisms

### Three Paradigms Discovered

#### Paradigm 1: Autoresearch (Karpathy) - Artifact Improvement Loop

```
while true:
  read current_artifact + results_history
  form hypothesis
  modify artifact
  git commit
  run evaluation (5 min, single metric)
  if improved: keep (new baseline)
  else: git reset --hard HEAD~1
  log to results.tsv
```

- Agent improves an EXTERNAL artifact (train.py), NOT itself
- Single metric (val_bpb), single mutable file, fixed time budget
- Git as substrate: commit = experiment, reset = rollback
- 100 experiments/night, ~$15 via Claude API
- Results: 11% training speedup (Karpathy), 19% improvement (Shopify CEO)

**Applicable to specpunk:** The ratchet loop pattern applies to ANY measurable improvement:
- `punk check` pass rate across tasks
- Cost per successful task
- Time to completion by category
- Receipt error rate

#### Paradigm 2: Hermes - Skill Self-Authoring + RL Training

**Skill creation trigger** (schema description, not code):
```
Create when: complex task succeeded (5+ tool calls), errors overcome,
user-corrected approach worked, non-trivial workflow discovered.
Update when: instructions stale/wrong, OS-specific failures,
missing steps found during use. Patch it immediately.
```

No external supervisor. The LLM reads the tool description and self-decides.

**Skill patching during use:**
- Every time a skill fails unexpectedly, the agent is prompted to `skill_manage(action='patch', ...)`
- Find-and-replace with validation: unique match required, frontmatter re-validated after patch
- Security scan + rollback on every write

**RL Training Pipeline (Hermes-specific, not applicable to us):**
1. `batch_runner.py` - runs AIAgent across JSONL prompts in parallel, generates ShareGPT trajectories
2. `trajectory_compressor.py` - prunes to 15K tokens, preserves training signal
3. `agentic_opd_env.py` (On-Policy Distillation) - extracts per-token teacher signals from tool results
4. Atropos-compatible environments for SWE-bench, terminal tasks, web research

**Key insight:** OPD creates DENSE token-level training signal from every tool call (not just sparse task-level reward). This is NousResearch's edge - they can fine-tune their own models.

**We can't do RL** (we use frontier models, not our own). But we CAN do skill self-authoring.

#### Paradigm 3: Superpowers - TDD for Prompts + Persuasion Engineering

**Skill creation = TDD:**
1. **RED:** Run pressure scenario WITHOUT skill. Document agent's rationalizations verbatim.
2. **GREEN:** Write minimal skill addressing ONLY observed rationalizations. Re-test.
3. **REFACTOR:** When agent finds new loophole, add explicit counter. Re-test.

**Iron Law:** "NO SKILL WITHOUT A FAILING TEST FIRST"

**Pressure scenario design:**
- Stack 3+ simultaneous pressures: time, sunk cost, authority, economic, exhaustion
- Force explicit A/B/C choice
- Use real file paths, real context
- Single-pressure tests are weak - agents resist one but break under multiple

**Persuasion engineering** (Cialdini's 7 principles, Meincke et al. 2025: compliance 33% -> 72%):
- **Authority:** "YOU MUST", "Never", "No exceptions" - eliminates decision fatigue
- **Commitment:** Force announcements, explicit choices, TodoWrite tracking
- **Social Proof:** "Every time", "Always" - establishes norms
- **AVOID:** Liking (creates sycophancy), Reciprocity (feels manipulative)

**3-fix escalation rule:**
- After 3 failed fixes: STOP. This is not a bug, it's wrong architecture.
- Forces learning: "each fix reveals new shared state/coupling = architectural problem"

**Verification-before-completion gate:**
```
BEFORE claiming any status:
1. IDENTIFY what command proves the claim
2. RUN the full command (fresh)
3. READ full output, check exit code
4. VERIFY: does output confirm claim?
5. ONLY THEN: make the claim
Skip any step = lying, not verifying
```

---

## Part 3: What Specpunk Should Adopt

### Memory Design

```
specpunk memory layers:

L0: Working Memory (per-task)
    = task.json + prompt + in-context
    = ephemeral, dies with the task

L1: Session Memory (per-project, persistent)
    = state/sessions/<project>/context.json
    = last N entries with TTL, typed (success/failure/surprise)
    = frozen snapshot injected at task start (Hermes pattern)
    = atomic writes (temp + mv)
    = scanned for injection before use

L2: Receipts (append-only, global)
    = receipts/index.jsonl
    = every task produces a receipt
    = the episodic memory layer
    = queryable by `punk ask` and `punk status`

L3: Engram (long-term semantic, external)
    = mem_save / mem_search via MCP
    = durable facts, decisions, procedures
    = already exists, don't duplicate

L4: Skills (procedural, persistent)
    = skills/*.md
    = how to do things
    = self-authored by agents after complex tasks
```

**No vector DB. No graph DB. No SQLite.**
- L1: JSON file per project (Hermes proved frozen snapshots work)
- L2: JSONL (we already use this pattern)
- L3: Engram MCP (external, already works)
- L4: Markdown files (already have)

**Letta benchmark justifies this:** filesystem + smart retrieval agent > fancy DB.

### Self-Improvement Design

Three loops, ordered by implementation difficulty:

#### Loop 1: Receipt-Driven Learning (Autoresearch pattern)
```
task completes -> receipt written -> session updated
  |
  v
session context carries:
  - last N receipts (with typed signals)
  - negative signal required (force capture of failures)
  - TTL on entries (stale facts auto-evict)
  |
  v
next task reads session -> agent has memory of what worked/failed
```

**Effort: Low.** Already building receipts + sessions. Just add typed entries + TTL.

#### Loop 2: Skill Self-Authoring (Hermes pattern)
```
agent completes complex task (5+ tool calls, errors overcome)
  |
  v
agent decides to create/patch skill
  |
  v
skill written to skills/<name>/SKILL.md
  with frontmatter: name, description, triggers
  |
  v
security scan (regex for injection + Unicode)
  |
  v
next task: skill available for agent to use
```

**Effort: Medium.** Need skill discovery in punk-run, security scan, atomic writes. But skills are just markdown files - no compilation needed.

#### Loop 3: Metric Ratchet (Autoresearch pattern, adapted)
```
weekly cycle:
  read receipts from last 7 days
  compute metrics:
    - punk check pass rate
    - avg cost per successful task
    - failure rate by category
    - model routing accuracy
  compare to previous week
  if degraded: emit directive
  if improved: log what changed
```

**Effort: Medium.** punk-watch (the Rust daemon replacing cycle-controller) runs watchers that compute these metrics. Directive emitted on regression.

#### Loop 4: Pressure-Testing Skills (Superpowers pattern, future)
```
when creating a skill:
  1. RED: run task WITHOUT skill, observe failure
  2. GREEN: write skill, re-run, observe success
  3. REFACTOR: if new failure mode found, patch skill

verification: skill must have at least one test scenario
```

**Effort: High.** Requires automated test harness for skills. Future work.

### What NOT to Build

| Temptation | Why Skip |
|-----------|----------|
| Vector DB for memory | Letta proved filesystem works. Complexity not justified at our scale. |
| Knowledge graph | Cool but massive engineering. Receipts + Engram cover our needs. |
| RL training pipeline | We use frontier models. Can't fine-tune Claude/Codex/Gemini. |
| On-Policy Distillation | NousResearch-specific. Requires own model. |
| Temporal KG (Zep/Graphiti) | Overkill for solo founder. Receipts have timestamps. |
| Honcho user modeling | We have one user. Not needed. |
