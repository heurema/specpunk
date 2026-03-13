---
title: "Spec-Driven Development: Deep Research (Second Pass)"
date: 2026-03-12
depth: deep
agents: 1
verification_status: verified-where-noted
completion_status: complete
sources: 60+
predecessor: 2026-03-12-spec-driven-development.md
---

# Spec-Driven Development: Deep Research (Second Pass)

This document is a second-pass deep dive. It does NOT repeat initial findings (CodeSpeak overview,
Tessl/Kiro/spec-kit surface level, Fowler's 3-tier taxonomy, Self-Spec, DDD). It goes deeper into
each tool's internals, examines the historical failure record, explores the design space, and probes
enterprise adoption reality.

---

## 1. CodeSpeak Internals

### 1.1 CLI command surface

`codespeak-cli` on PyPI (package: `codespeak_cli-0.3.5-py3-none-any.whl`, 30.6 KB, no external
dependencies). Three maintainers: `abreslav`, `dsavvinov`, `ksafonov`. Released from private repo
`codespeak-dev/open-codespeak` via Trusted Publishing (GitHub Actions). 16 releases between
2026-02-10 and 2026-03-10 — rapid iteration cadence (~1 release per 2 days in peak periods).

**Complete known command set** (from tutorials + blog posts):

```
codespeak login                          # Google/email auth (BYOK: Anthropic API key)
codespeak init                           # new greenfield project
codespeak init --mixed                   # mixed mode: CodeSpeak manages subset of files
codespeak build                          # compile specs → code
codespeak build --skip-tests             # build without test generation
codespeak change -m "description"        # implementation-level bug fix, not spec change
codespeak change --spec <path> -m "..."  # multi-spec projects: disambiguate target spec
codespeak takeover <file_path>           # extract spec from existing source file (~43s)
CODESPEAK_ANTHROPIC_STANDARD_MODEL=claude-opus-4-6 codespeak build  # model override
```

**Model handling:** CodeSpeak is not model-agnostic by design — it requires Anthropic API key
(BYOK). Default model is unspecified in docs; complex mixed-mode projects "work best with
claude-opus-4-6", set via env var `CODESPEAK_ANTHROPIC_STANDARD_MODEL`. Model version changes
are a known problem: the roadmap acknowledges future equivalence validation as incomplete work.

### 1.2 codespeak.json structure

Two top-level arrays control what CodeSpeak manages:

```json
{
  "specs": ["path/to/feature.cs.md"],
  "whitelisted_files": [
    "src/module/__init__.py",
    "src/module/_main.py",
    "tests/_test_vectors.py"
  ]
}
```

- `specs`: paths to `.cs.md` files that CodeSpeak owns and compiles
- `whitelisted_files`: existing project files CodeSpeak may modify (integration points)
- Files not in either list are untouched in mixed mode
- JSON errors now surface a readable error message rather than a silent crash (added in 0.3.x)

### 1.3 .cs.md format in detail

Two distinct formats depending on mode:

**Greenfield (spec/main.cs.md):**
```markdown
# Project Name

One-paragraph description of overall purpose.

## UX

- Feature bullet 1
- Feature bullet 2 (natural language, no tech detail)

## Technology

- Django backend
- Tailwind CSS
- HTML templates
```
Typical length: 15–30 lines for a small app. No API schemas, no database design, no imports.

**Mixed mode / takeover (.cs.md alongside source):**
```markdown
# ComponentName

High-level description of what this component does.

## Accepts

- `.ext1`, `.ext2`, `mime/type` inputs

## Output Structure

Markdown with section headers for each part.

## Parsing Requirements

- Specific technical detail (stream IDs, protocol constants, etc.)
- Edge cases that require implementation knowledge
```

The `codespeak takeover` command reads existing source and produces a `.cs.md` at ~10x compression
(the compression ratio is the proof-of-concept claim). The spec omits what can be inferred; it
preserves what only a human with domain knowledge would write.

**change-request.cs.md:** A separate escape hatch. Created via `codespeak change -m "..."` or
manually. Describes an implementation-level fix without changing the spec. CodeSpeak applies the
fix and discards the change-request file. Used for bugs that are "too low-level for the spec."

### 1.4 Code Takeover: step-by-step

1. Run `codespeak init --mixed` in existing project root
2. Run `codespeak takeover path/to/file.py` (example took 43 seconds for a mid-size file)
3. CodeSpeak reads the source, calls LLM to extract a compressed spec
4. Writes `_filename.cs.md` next to the source file
5. Registers the spec in `codespeak.json`
6. Developer edits the spec (adding what was missing, removing what was wrong)
7. `codespeak build` regenerates code from the edited spec
8. Whitelisted files (e.g., `__init__.py`, test files) are updated to integrate the new module

The prompts used internally are not disclosed. The extraction step is the critical IP.

### 1.5 Equivalence checking: current state and roadmap

**Current (0.3.x):** Partial. CodeSpeak generates test vectors from declarations in `.cs.md` specs
(e.g., "Test input declares expected characteristics") and runs them during build. In the yt-dlp
case study: before takeover 1241/1242 tests pass; after 1278/1279 pass (37 tests *added* because
the spec captured missing cases). Equivalence is operationally defined as "all existing tests pass
plus new ones added by spec."

**Roadmap (explicit):** "Making sure that if we delete the code, an equivalent implementation can
be generated from the spec (passing all the tests)" is listed as future work. This is the spec
diff → code diff problem — currently unsolved. Model version changes could break regeneration
without the developer noticing (silent regression).

### 1.6 Release velocity and stability signals

```
0.2.0  2026-02-10  (initial public)
0.2.10 2026-02-19  (9 patch releases in 9 days)
0.3.0  2026-02-23  (minor bump, same day as Takeover blog post)
0.3.5  2026-03-10  (latest; git index.lock contention fix, multi-spec change request fix)
```

Git index.lock contention bug (fixed in recent release) indicates real-world users running
CodeSpeak alongside other git clients (IDEs, hooks). Python >=3.13 required — above-average
minimum version constraint, limits adoption on legacy infra.

---

## 2. Tessl Deep Dive

### 2.1 Company and funding

- Founded 2024 by Guy Podjarny (ex-Snyk CEO, ex-Akamai CTO)
- $25M seed (GV/boldstart, April 2024) + $100M Series A (Index Ventures, November 2024)
- Valuation: $750M (November 2024)
- Products launched September 23, 2025

### 2.2 Two products: Framework + Registry

**Tessl Framework** (closed beta as of launch):
- MCP server integration — agents interact via MCP tools rather than direct file edits
- Creates three resource types before implementation: Plans, Specs, Tests
- Spec-per-code-file mapping: one spec for each source file it manages
- Annotations in generated code: `// GENERATED FROM SPEC - DO NOT EDIT`
- Tags in specs: `@generate` (triggers code generation) and `@test` (links to test)
- API section in each spec: documents the file's exported interface

**Tessl Spec Registry** (open beta, free):
- 10,000+ "Usage Specs" as of launch (September 2025)
- Version-accurate: each spec is pinned to a library version (like npm packages)
- Installable as dependencies — similar mental model to package managers
- Organizations can publish private specs for internal libraries/policies
- Purpose: prevent API hallucinations and version mixups by grounding agents in accurate docs

### 2.3 .spec.md format (detailed)

YAML frontmatter + Markdown body:

```markdown
---
name: User Authentication
description: Login and session management
targets:
  - ../src/auth/*.py
---

## Capabilities

- [Description of what this module does]
- [@use other-spec] for importing capabilities from another spec

## API

```python
def authenticate(username: str, password: str) -> Session: ...
```

## Requirements

- WHEN user provides valid credentials THE SYSTEM SHALL return a Session object
- [@test test_auth.py::test_valid_login]

## Implementation Notes

- [Optional: @generate tag triggers code generation for this section]
```

Key fields:
- `targets`: glob patterns or file paths this spec describes (enables automated linking)
- `[@use spec-name]`: imports another spec's capabilities (composition)
- `[@test path::test_name]`: inline link to a test that verifies this requirement
- `@generate`: marks sections for code generation
- Implementation notes with `.impl` notation for lower-importance details (per expert interviews)

### 2.4 Non-determinism: Tessl's actual mitigation strategy

Non-determinism is acknowledged as a two-layer problem:
1. **Model drift**: providers ship new model versions that change behavior under identical specs
2. **Stochastic inference**: same model, same spec, different outputs across runs

**Tessl's approach (multi-layer):**

Layer 1 — Spec grounding: Structured specs reduce output variance by constraining the solution
space. Empirically: "much more deterministic" per practitioners, but not deterministic.

Layer 2 — Harbor (statistical evaluation): Rather than binary pass/fail, Harbor measures pass
rate over N trials. The question becomes "what's the pass rate over N trials?" not "did it pass?"
- Task structure: `instruction.md` + `tests/test.sh` + `task.toml` + `environment/`
- Tests write numerical rewards (0 or 1) enabling statistical aggregation
- Supports parallel execution: 100 containers simultaneously (Daytona, E2B)
- SWE-Bench-style: leading agents now clear 70%+ pass rates via standardized targets
- Controlled experiment: 30 trials × 3 configurations = 90 parallel runs, completed in <5 min

Layer 3 — CI/CD for context: Three-tier eval methodology:
- Review evals: deterministic checks against specific criteria
- Task evals: isolated agent behavior validation
- Project evals: full codebase integration testing
- Error budgets (not pass/fail): pre-defined acceptable failure rates per eval type
- Scheduled independent runs detect context staleness from external changes

Layer 4 — Spec evolution contract: Humans retain authority over test failures. Only humans can
authorize a regression test failure. Tests become locked-in canonical specs.

**Practical result (from controlled experiment):**
- Vanilla Claude Code (no skills): 53% pass rate (baseline)
- Official Atlas skill: 73% (+20%)
- Custom project skill: 80% (+27%)
- Custom skill had 96% pass rate when invoked, 0% when not invoked
- Activation rate improvement by stronger language: ~10% → 57–83%

### 2.5 Registry entry structure: spec-driven-development tile

The `tessl-labs/spec-driven-development` tile in the registry (v1.0.5, marked "Latest" with
"Pending" quality evaluation) implements a four-stage workflow:

1. **Clarifying Questions**: Interview user about requirements, one question at a time
2. **Specification Creation**: Structured spec docs precede any code
3. **Approval Gate**: Pause for human confirmation specs match intent
4. **Guided Implementation**: Build against approved specs with verification

Source: `https://github.com/tesslio/spec-driven-development-tile/`

---

## 3. Historical Failures: What Spec-Driven Approaches Died and Why

### 3.1 Model-Driven Architecture (MDA, 1990s–2010s)

**What it promised:** Write platform-independent models (PIMs) in UML; tools generate
platform-specific models (PSMs) and then code. Specs as source.

**Why it failed** (Fowler's analysis + subsequent literature):

1. **Behavioral specification gap**: UML diagrams can capture structure (class/component) but
   struggle with behavior (algorithms, business rules, control flow). Sequence diagrams and
   activity diagrams are not as expressive as modern programming languages.

2. **The formality cliff**: UML was designed for sketching ideas. Making it formally complete
   enough for reliable code generation required massive additional notation (OCL, action
   semantics) that practitioners refused to adopt.

3. **CASE tool redux**: The same promises were made for CASE tools in the 1980s. Both failed
   because they couldn't produce a coherent environment more effective than writing code directly.

4. **QVT standardization failure**: The OMG's model transformation language (QVT) had no complete
   implementation, no industrial support, and was not used in practice.

5. **EMF scalability**: Eclipse Modeling Framework doesn't scale for large models and doesn't
   provide workgroup support.

6. **Pragmatic MDA emerged instead**: Industry acknowledged full MDA was too idealistic and
   adopted a "pragmatic MDA" where models generate boilerplate scaffolding (DAOs, DTOs, REST
   endpoints) but developers write the real logic. This is the "spec-anchored" level — not the
   "spec-as-source" aspiration.

7. **Visual superiority myth**: No empirical evidence that pictures are better than text for
   expressing logic. Fowler: "I can't see that drawing sequence diagrams or activity diagrams
   is as good, let alone better, than writing code in a modern language."

**Current state**: MDA is not dead — MathWorks Simulink/Stateflow is used by 95% of automotive
OEMs (per internal MathWorks data) and all top-10 aerospace companies for safety-critical embedded
systems (ISO 26262, DO-178C). The key difference: these domains have formal semantics (control
systems, state machines) and accept the constraint that models ARE the spec. General enterprise
software never accepted this constraint.

### 3.2 TLA+ and Formal Specification

**Status** (systematic literature review, IFM 2024, 290 papers → 16 high-affinity):

- Documented users: Amazon, Microsoft, MongoDB, Huawei, Alibaba
- Domain concentration: Cloud infrastructure (63%), railway/control systems (13% each)
- Primary use: Early design phase (81%), debugging (44%), implementation (38%)
- Language preference: Pure TLA+ (50%), TLA+ + PlusCal combined (31%)
- Growth: Significant upward trend (S=3.67, p<0.001) since 2015, Amazon paper as catalyst

**Why TLA+ is narrow despite success:**

1. Steep learning curve — "modeling existing systems remains an effortful task that threatens
   scalability"
2. Only 19% of papers address automated synchronization between specs and production code
3. Model-implementation gap: spec proves properties of an abstraction, not the actual code
4. State-space explosion for complex systems
5. Abstraction selection is hard: too detailed = state explosion; too abstract = no value

**CLEVER benchmark** (LLM + formal verification, 2025): LLMs can solve only 1/161 end-to-end
verified code generation problems. Verification-only tasks: 89% success with Claude 3.5 Opus,
96% with newer models. Gap between "write code" and "write provably correct code" is vast.

### 3.3 FIT / FitNesse (executable specifications, 2001–present)

**What it was:** Ward Cunningham's Framework for Integrated Test. Business analysts write
acceptance tests as HTML tables; Fixtures connect them to production code. FitNesse added a wiki
wrapper. Promised: "business-readable, developer-verifiable specifications."

**What happened:**
- FIT (the original) is effectively obsolete, no further development
- FitNesse maintains community-driven development (latest release v20250223)
- Adoption: stable maintenance mode, not growth

**Why it didn't scale:**
- HTML table syntax felt technical to non-technical stakeholders
- Setting up Fixtures required developer effort defeating the collaboration promise
- Competing approaches (Cucumber/Gherkin) offered more natural language
- Living documentation concept migrated to Cucumber's scenario model

### 3.4 BDD / Gherkin at scale: the documented failure modes

~27% of open-source projects use BDD frameworks (survey data). Of those, many use Cucumber for
test automation rather than for the behavior specification + business collaboration it was
designed for.

**The "scenario explosion" cliff** (no canonical "500" number, but documented at enterprise scale):

- Feature files become bloated as applications grow
- Scenarios start to conflict or overlap
- The "simple English" syntax that was supposed to enable non-technical participation rarely does
  in practice: "feature files holding Gherkin scenarios can never truly be read and written by
  everyone on a team" (Cucumber's own documentation)
- Living documentation degrades: teams stop updating feature files after initial implementation
- BDD tools get used as test automation frameworks, not as specification tools

**2024 World Quality Report:** 60%+ of agile teams adopted BDD practices. BDD tool market
valued at $445.6M (2024), projected $689.4M by 2030. Yet satisfaction data is mixed.

**Documented failure pattern:**
- Phase 1: Enthusiastic adoption, collaboration improves
- Phase 2: Feature file count grows, specialist "automation engineers" own them
- Phase 3: Non-technical stakeholders stop reading/writing; files become test scripts disguised
  as Gherkin
- Phase 4: Maintenance burden grows; teams quietly abandon BDD while keeping Cucumber as test
  runner

**Cucumber's own postmortem list:** "10 easy ways to fail at BDD" (published on cucumber.io)
lists neglecting to keep living documentation updated as the first and most common failure.

### 3.5 Spec-Kit practical critique (Scott Logic hands-on, November 2025)

**Measured results** (two features, real project):

| Approach | Agent Time | Code Lines | Markdown Lines | Review Time |
|---|---|---|---|---|
| Spec-Kit | 57 min | 989 | 4,839 | 5.5+ hours |
| Iterative (no spec) | 8 min | 1,000 | 0 | 24 min total |

Speed differential: Spec-Kit was ~10x slower end-to-end.

**Specific failure modes observed:**
- Simple bug (variable not populated) slipped through despite specification-driven approach
- Unclear workflow for fixing bugs after spec-driven implementation
- First feature alone generated 2,577 lines of markdown (444-line module contract, 395-line data
  model, 285-line implementation plan, 500-line quick-start guide)
- Agent marked "verify implementation" task as done without writing a single unit test —
  wrote manual testing instructions instead
- Agent generated duplicate classes, ignoring its own specifications

**Core critique:** "Code is now cheap; we can create it quickly and throw it away just as fast.
Spec Kit, and SDD, don't capitalise on this." The spec-as-documentation overhead offsets the
productivity gains from AI code generation.

**Waterfall parallel:** SDD's sequential Constitution → Specify → Plan → Tasks → Implement
mirrors waterfall's sequential stages despite agile framing.

---

## 4. Spec Format Design Space

### 4.1 Format options and tradeoffs

| Format | Strengths | Weaknesses | Best for |
|---|---|---|---|
| Markdown (.md, .cs.md, .spec.md) | Human readable, git-diffable, IDE support, no tooling | No schema enforcement, free-form, hard to parse programmatically | Agent-consumed specs, living docs |
| YAML frontmatter + Markdown body | Structured metadata + human-readable body, machine-parseable header | YAML syntax friction, indent errors | Registry-style specs with targets/metadata |
| OpenAPI / AsyncAPI | Machine-executable, code generation, contract testing | Verbose, technical, non-technical stakeholders excluded | API contracts, SDK generation |
| EARS (textual) | Lightweight, no tools needed, structured enough to be testable | Verbose pattern repetition, learning curve, not machine-parseable without tooling | Requirements engineering, acceptance criteria |
| Gherkin (Given/When/Then) | Business-readable, executable, test framework integration | Doesn't scale to complex logic, over-engineered for simple cases | BDD scenarios, acceptance tests |
| TypeSpec (DSL) | Strongly typed, generates OpenAPI/JSON Schema, TypeScript-inspired | Microsoft-ecosystem, steep learning curve, less human-friendly | API-first enterprise (Azure ecosystem) |
| Formal (TLA+, Alloy, Dafny) | Provably correct, model-checkable | Expert-only, doesn't integrate with most workflows | Safety-critical, distributed systems |
| Model-based (Simulink) | Spec IS the executable, direct code generation, certified tools | Domain-restricted (control systems), expensive tooling | Automotive, aerospace embedded |

### 4.2 Acceptance criteria: format comparison

**EARS (Easy Approach to Requirements Syntax)** — published 2009, IEEE:
```
Pattern:          While <precondition>, when <trigger>, the <system> shall <response>
State-driven:     While [state], the system shall [behavior]
Event-driven:     When [event], the system shall [response]
Optional feature: Where [feature], the system shall [behavior]
```
Strengths: lightweight, no specialist tools, low training overhead, widely readable.
Weaknesses: verbose pattern repetition for complex requirements, not machine-parseable without
a parser. Used by Kiro natively.

**Given/When/Then (Gherkin):**
```gherkin
Given a user with valid credentials
When they submit the login form
Then they should receive an authentication token
```
Strengths: natural language, directly executable via Cucumber/SpecFlow/Behave.
Weaknesses: doesn't scale to conditional logic, edge case explosion, scenario maintenance burden.

**Bullet points (CodeSpeak style):**
```markdown
- Support OAuth2 with Google
- Handle expired sessions with 401 + redirect
- Rate limit to 100 requests/minute per user
```
Strengths: minimal, fast to write, 5–10x shorter than EARS/Gherkin.
Weaknesses: no formal structure, agent interprets ambiguity, equivalence harder to test.

**Research finding (ICSE 2026, peer-reviewed):** Incorporating architectural documentation
substantially improves LLM-assisted code generation — measurable gains in functional correctness,
architectural conformance, and code modularity. The "curse of instructions" effect: as more
instructions are added, per-instruction adherence drops significantly even for GPT-4 and Claude.

### 4.3 Optimal spec granularity

No controlled study directly comparing function/module/feature-level specs for AI code generation
exists in the literature. Practitioner consensus (from 2,500+ agent config file analysis):

**Six essential spec sections** (Addy Osmani analysis):
1. Commands (exact executable commands with flags)
2. Testing (framework, test file locations, coverage expectations)
3. Project Structure (explicit paths)
4. Code Style (one real code snippet beats three paragraphs of description)
5. Git Workflow (branch naming, commit format, PR requirements)
6. Boundaries (three-tier: Always do / Ask first / Never do)

**Granularity guidelines:**
- Too coarse: "Build a user management system" — agent fills gaps with assumptions
- Too fine: Function-level specs recreate the implementation in English, defeat the purpose
- Optimal: Feature-level or module-level (one spec per feature/component)
- Break at natural boundaries: separable responsibilities, clear interfaces, bounded contexts

**The "curse of instructions" research implication:** A spec with 50 requirements adheres to all
50 worse than 5 targeted specs with 10 requirements each. Context decomposition > monolithic spec.

**Tessl's finding:** spec-per-code-file mapping (1:1) is their current level. This is fine-grained
but ensures the spec stays local to its implementation — prevents spec drift via proximity.

### 4.4 Cross-cutting concerns in specs

Cross-cutting concerns (auth, logging, error handling, caching, security) are the hardest to spec
because they span multiple modules. Known patterns:

**Pattern 1: Global steering files (Kiro model)**
Steering files with `inclusion: always` mode establish cross-cutting rules globally:
```yaml
---
inclusion: always
---
# Auth Standards
All endpoints must validate JWT tokens via `auth.validate_token()`.
Return 401 with `{"error": "unauthorized"}` on failure.
```
Domain-specific steering files activate via `fileMatch` patterns:
```yaml
---
inclusion: fileMatch
fileMatchPattern: "src/api/**/*.py"
---
# API Error Handling
All API handlers must catch exceptions and return structured error responses.
```

**Pattern 2: Spec composition via @use (Tessl model)**
```markdown
[@use auth-spec]
[@use error-handling-spec]
```
Cross-cutting specs become importable dependencies. Spec Registry enables sharing across teams.

**Pattern 3: Constitution / project-context.md (BMAD model)**
A single "constitution" document defines project-wide principles. All agent interactions load
it. Cross-cutting rules live here, not in individual feature specs.

**Pattern 4: Inheritance via layers (OpenAPI approach)**
Common components defined in shared schemas (`components/securitySchemes`, `components/responses`)
referenced by individual endpoint specs via `$ref`. Not markdown-native but effective for APIs.

**Unresolved challenge:** When a cross-cutting concern changes (e.g., auth library upgrades), all
specs that reference it must update. No current tool provides automated cross-reference propagation.

---

## 5. Spec Versioning and Evolution

### 5.1 The spec drift problem (measured)

**API drift (OpenAPI context):**
- 75% of APIs don't conform to their specifications (cited survey)
- Only 10% of organizations fully document their APIs (EMA 2023)
- Drift is driven by: time pressure, unclear ownership, informal changes (Slack decisions), and
  code-first development where specs are written after the fact

**Spec drift in AI-driven development:**
- Spec drift occurs when implementation fixes are made outside the spec workflow
- "Run `/opsx:verify` to detect when implementation contradicts artifacts" (OpenSpec)
- AI agents documented as generating code contradicting their own specifications
- Kiro identified: "Currently specifications are mostly static documents" that "easily get out
  of sync with the actual state of the code as changes are made"

### 5.2 Git strategies for spec + code co-evolution

**Practitioner consensus** (from github/spec-kit discussion #152, OpenSpec documentation):

**Strategy 1: Immutable specs + compaction**
- Never mutate original specs; changes create new version specs
- Periodically run "compaction" or "rollup" commands to merge spec fragments into a
  coherent snapshot
- Git history shows: spec-v1.md → spec-v2.md → spec-snapshot-20260312.md
- Problem: cognitive overhead — understanding current state requires reading multiple files

**Strategy 2: Master spec + diff archive**
```
openspec/
  specs/           # master spec (current state)
  changes/
    active/        # work-in-progress change folders
    archive/       # completed changes for audit trail
```
When `/opsx:archive` completes, deltas merge into `specs/` and change folder moves to `archive/`.
Provides both single-source-of-truth and chronological audit trail.

**Strategy 3: Target-state declarations**
Specs describe desired outcomes, not deltas. Agent calculates what changed.
Requires advanced diffing infrastructure — closest to spec-as-source vision.

**The pragmatist view** (key quote from spec-kit discussion): "LLMs should read relevant code
for current status, not potentially outdated specifications. Specifications should describe
changes to existing state." This inverts the spec-as-truth assumption: code is truth, specs
document intent and track changes.

**OpenSpec workflow** (most detailed practitioner-documented approach):
```
.openspec.yaml     # metadata: change status, timestamps
proposal.md        # motivation, scope, constraints, acceptance criteria
specs/             # functional requirements marked ADDED/MODIFIED/REMOVED
design.md          # data model, APIs, architecture, tech choices
tasks.md           # implementation steps in ~30-minute chunks
```
Commands: `/opsx:new` → `/opsx:ff` (fast-forward) → `/opsx:apply` → `/opsx:archive`
Verification: `/opsx:verify` (reports missing requirements coverage, pattern deviations)
Mid-change sync: `/opsx:sync` (merge deltas without archiving)

### 5.3 Spec migration strategies

**Problem:** Spec format changes (e.g., adopting EARS notation, switching from YAML to Markdown
frontmatter) require migrating all existing specs. No current tool provides automated spec
migration.

**API specification versioning (TypeSpec / OpenAPI):**
- Semantic versioning applied to specs: breaking changes → major bump
- Machine-readable compatibility classification (auto-generated on spec diff)
- Codemods can cover some breaking changes automatically
- Date-based versioning (YYYY-MM-DD) provides no compatibility signal — being replaced by SemVer
  (e.g., MCP specification)

**Practical approaches for non-API specs:**
1. Manual migration with sed/awk scripts when format is structured enough
2. LLM-assisted migration: "convert all .spec.md files from v1 to v2 format using this template"
3. Gradual migration: old + new formats coexist, new specs use new format, old specs migrate on
   edit
4. Freeze + rewrite: accept old specs as read-only artifacts, rewrite from scratch in new format

**Key finding:** Spec migration is analogous to database migrations — the tooling is immature
compared to the code migration ecosystem (codemods, AST transforms). This is a significant gap.

### 5.4 Continuous spec validation in CI

**What exists today:**

For API specs (OpenAPI):
- `Spectral` (Stoplight): Rule engine for OpenAPI/AsyncAPI linting; custom rules via JS
- `oasdiff`: Diff and breaking change detection for OpenAPI; CI integration
- `Dredd`: HTTP contract testing against OpenAPI specs (validates code against spec at runtime)
- `Optic`: CI/CD conformance checks, drift detection
- Pactflow/Karate: Contract testing frameworks

For general markdown specs:
- No standard tooling — this gap is unaddressed
- Custom scripts that check spec structure (required sections, link validity)
- Tessl's Harbor: eval-based validation (statistical, not deterministic)

**Tessl's CI/CD model for context:**
- "Error budgets, not pass/fail" — pre-define acceptable failure rates per eval type
- Scheduled independent evaluation runs catch staleness from external changes
- Real-world feedback loop via Langfuse observability

**Emerging pattern:** Running Harbor tasks as CI jobs. Each PR triggers: structural validation
(`harbor tasks check`) + oracle execution (must pass 100%) + AI-detection screening.

---

## 6. Enterprise Adoption Patterns

### 6.1 Actual company implementations (verified data)

**Simulink/MathWorks (model-based, spec-as-source for embedded systems):**
- 95% of automotive OEMs use MATLAB/Simulink (MathWorks internal data)
- 100% of top-10 aerospace companies use it
- ISO 26262 (automotive safety) and DO-178C (aerospace software) mandates model-based approaches
- TÜV SÜD-certified tools for ASIL A-D safety levels
- This is the only proven large-scale spec-as-source deployment in industry — but domain-specific

**Amazon (TLA+):**
- Documented in Newcombe et al. (2015), cited by 44% of all industrial TLA+ papers
- Used for: DynamoDB, S3, distributed systems protocol verification
- Identified critical bugs in distributed protocols that testing missed
- Adopted after evaluating Alloy first
- Not spec-as-source: TLA+ specs are design-phase tools, not code generators

**Twilio (OpenAPI spec-first):**
- Modular OpenAPI specs — one per API subdomain (api.twilio.com, accounts.twilio.com, etc.)
- Auto-generated and validated against production API behavior
- Used to generate SDKs and mock APIs
- Internal pipeline: spec → validation → SDK generation → docs
- Organization: separate repositories per API domain

**Stripe (OpenAPI, code-first approach):**
- Single monolithic OpenAPI (auto-generated from code, not maintained as spec)
- Team explicitly chose not to break it into modular specs ("little interest")
- The spec froze most applications when imported — usability issue at scale
- Lesson: code-generated specs drift less (they're always in sync) but aren't spec-first

**Microsoft (TypeSpec for Azure):**
- TypeSpec (formerly ADL/Cadl, 2019→2023 rename) now at 1.0 GA
- Used by Azure services and Microsoft Graph teams
- Generates OpenAPI from TypeSpec → then generates SDKs, docs, mocks
- Adoption outside Microsoft ecosystem remains limited

### 6.2 Tooling landscape stars and adoption signals

| Tool | Stars | Forks | Status |
|---|---|---|---|
| spec-kit (GitHub) | 39.3k | — | v0.1.4, agent-agnostic, 13+ AI assistants |
| BMAD-METHOD | 19.1k | 2.8k | v4.x stable, v6-alpha in progress |
| Kiro (AWS) | IDE | — | Released mid-2025, preview |
| OpenSpec | 4.1k | — | Brownfield-focused, frequent updates |
| PromptX | 3k | — | MCP-native, persona model |
| GTPlanner | 122 | 57 | Emerging |
| CodeSpeak | — | — | Alpha, 16 releases in 29 days |
| Tessl | — | — | $750M valuation, open beta registry |

### 6.3 ROI metrics: what's verified vs what's marketing

**Verified (controlled experiments):**
- Tessl controlled experiment: +20% pass rate with official skill, +27% with custom skill (30 trials)
- Tessl LangGraph evaluation: 8.2%–20.4% improvement range across features and models
- Tessl's context evaluation: ~35% improvement in proper API usage with specs vs baseline
- BMAD: 55% faster completion rate (cited, source: internal claim, limited verification)
- Red Hat study: 40% defect density reduction, 30% delivery velocity increase (cited in secondary
  sources, original study not independently verified)

**Unverified / marketing claims:**
- "80% fewer defects, 5-10x faster delivery" (SoftwareSeni article — no methodology)
- "95% accuracy in implementing specs on the first go" (Red Hat article — no empirical backing)
- "20-30% efficiency improvement per developer" (SoftwareSeni adoption playbook — prescriptive)
- "4-10x faster with predictable quality" (Zencoder Zenflow — no case studies)

**The Scott Logic finding (independently measured):** Spec-Kit was 10x *slower* end-to-end than
iterative development without specs for two comparable features.

**Y Combinator 2025 signal:** 25% of YC cohort ships codebases that are 95% AI-generated.
Specification quality cited as the differentiator between successful projects and technical
debt accumulation — but no controlled comparison.

### 6.4 Adoption friction: what actually blocks teams

**Four documented friction categories** (enterprise scale):

1. **Jira/ADO integration gap:** Most enterprises have months of refined backlog in Jira or
   Azure DevOps. Current SDD tools don't integrate with these systems. Teams can't adopt SDD
   without abandoning existing backlog investments.

2. **Multi-repo complexity:** Enterprise features span microservices, shared libraries, and
   infrastructure repos. No current SDD tool provides clear guidance on where specs live when a
   feature touches 6 repositories.

3. **Cross-functional participation:** Specs in code repos create barriers for PMs, BAs, and
   designers — the people who should define "what." Current tools are developer-centric.

4. **Workflow irreversibility:** Kiro's note that "workflows cannot be changed mid-project" is
   an extreme case of adoption lock-in. More generally, SDD requires upfront commitment that
   conflicts with iterative/agile discovery.

**Adoption timeline (practitioner-reported, not empirically validated):**
- Individual developer: 2-3 weeks to effective specification writing
- Small team (10-15): 90-day phased rollout framework (Days 1-30: pilot, 31-60: expansion,
  61-90: org-wide)
- Training investment: 8-12 hours over 4 weeks per developer
- Critical threshold: 70%+ adoption by pilot completion predicts successful rollout
- Inflection point: 25 developers (informal approaches break down, need program management)
- Champion network: 1 champion per 5-8 developers

**Cost structure (25-developer team, 90-day rollout):**
- Tool licensing: $20-40/dev/month
- Training: $800-1,200/developer
- Materials: $2,000-4,000 upfront
- Support: 0.5-1 FTE during rollout
- Total: $25,000-35,000

**Payoff:** ROI breaks even at 10+ developers (equivalent to adding 0.2-0.3 FTE per developer
in capacity). Annual tool cost ~$6,000-9,000 for 15-developer team.

### 6.5 The "waterfall strikes back" critique

Multiple independent practitioners (Marmelab, Scott Logic) identify SDD's core tension: it
re-introduces sequential, upfront design that agile deliberately rejected.

**Marmelab's argument (November 2025):**
- SDD agents "often miss existing functions that need updates"
- Specs generate "repetitions, imaginary corner cases, and overkill refinements"
- Double review burden: review the spec + review the code
- "For large existing codebases, SDD is mostly unusable"
- "80% of your time reading instead of thinking"
- Built a complex 3D sculpting tool "in about 10 hours" without specs

**Fowler's historical warning:** "spec-as-source...might end up with the downsides of both MDD
and LLMs: Inflexibility AND non-determinism."

**Where SDD clearly does NOT work:**
- Quick bug fixes (overhead > benefit)
- Exploratory prototyping (requirements unknown)
- Highly visual work (hard to spec)
- Large brownfield codebases (spec extraction is incomplete)
- Rapidly shifting requirements (spec becomes stale faster than code)

**Where SDD appears to deliver value:**
- Greenfield projects with clear requirements
- MVPs where cutting timelines in half makes previously unviable work feasible
- Safety-critical embedded systems (Simulink model — proven for 30+ years)
- API contract management (OpenAPI spec-first — proven at Twilio scale)
- Multi-agent coordination (spec as shared context between agents)
- Onboarding (spec as persistent memory: "reduces onboarding from months to days")

---

## 7. The Unsolved Problems

### 7.1 Spec diff → code diff (still unsolved)

This is the core unresolved problem across all tools. When a spec changes:
- Full regeneration is expensive, loses manual edits, and may break unrelated functionality
- Targeted patching requires understanding which spec change corresponds to which code change
- No tool implements reliable differential regeneration

**CodeSpeak claims:** diff-based code updates rather than full regeneration — but this is
incompletely implemented (see §1.5 equivalence checking roadmap).

**Tessl approach:** Lock in tests first; only regenerate code within test-bounded scope.
Still requires human oversight for regression.

**OpenSpec approach:** Tasks.md in 30-minute chunks; each chunk is independently regeneratable.
Changes trigger reruns of affected chunks only. Closest to practical differential regeneration.

**Formal methods gap:** Only 19% of TLA+ papers address spec-code synchronization. The model-
implementation gap is an acknowledged research priority, not a solved problem.

### 7.2 Cross-cutting concerns propagation

When auth spec changes → all specs that import `[@use auth-spec]` should update → downstream
code should update. No tool automates this cascade. Requires:
- Spec dependency graph
- Impact analysis (which specs reference the changed spec)
- Differential propagation of the change
This is the spec equivalent of dependency management — npm for specs.

### 7.3 Spec coverage measurement

For code, we have line/branch/condition coverage. For specs, we have no equivalent. How do you
know if a spec is "complete"? How do you know which parts of the spec are covered by tests?
Tessl's `[@test]` links provide manual linkage but no automated coverage analysis.

### 7.4 Spec quality metrics

What makes a spec good? Current answers are qualitative:
- "Not too vague" (Osmani)
- "Not too detailed" (Tessl practitioners)
- "5-10x shorter than code" (CodeSpeak)
- "One real code snippet beats three paragraphs" (Osmani)

No quantitative spec quality metric exists (analogous to cyclomatic complexity for code).
The closest: Tessl's eval pass rates as a proxy for spec quality.

---

## 8. Key Data Points and Numbers

| Fact | Value | Source | Confidence |
|---|---|---|---|
| API non-conformance rate | 75% of APIs don't conform to their specs | Survey (secondary) | Medium |
| BDD adoption | 27% of open-source projects use BDD frameworks | Survey | Medium |
| TLA+ industrial growth | S=3.67, p<0.001 upward slope since 2015 | IFM 2024 (peer-reviewed) | High |
| TLA+ spec-code sync | Only 19% of papers address it | IFM 2024 (peer-reviewed) | High |
| Tessl registry | 10,000+ usage specs | Tessl launch blog | High |
| BMAD stars | 19.1k stars, 2.8k forks | GitHub | High |
| spec-kit stars | 39.3k stars | GitHub | High |
| Tessl funding | $125M ($25M seed + $100M Series A) | Fortune Nov 2024 | High |
| Tessl valuation | $750M | Fortune Nov 2024 | High |
| Spec-Kit vs iterative | 10x slower end-to-end | Scott Logic (independent) | High |
| Tessl skill experiment | +27% pass rate (custom skill vs none) | Tessl blog (controlled) | High |
| Tessl context eval | ~35% better API usage with specs | Tessl blog | Medium |
| BMAD speed claim | 55% faster completion | BMAD self-report | Low |
| Red Hat defect claim | 40% defect reduction | Secondary sources | Low |
| LLM formal verification | 1/161 end-to-end verified code problems solved | CLEVER benchmark | High |
| Simulink automotive | 95% of OEMs | MathWorks internal | Medium |
| Simulink aerospace | 100% of top-10 | MathWorks internal | Medium |
| CodeSpeak releases | 16 releases in 29 days (Feb-Mar 2026) | PyPI | High |
| CodeSpeak Python req | >=3.13 | PyPI | High |
| YC 2025 AI codebases | 25% of cohort: 95% AI-generated | Marvin Zhang (secondary) | Low |

---

## 9. Synthesis: What the Second Pass Reveals

**The equivalence problem is the crux of SDD's future.** Every tool claims "spec as truth" but
none can prove that generated code is equivalent to the spec without running tests. The moment
you need tests to validate the spec, the spec is no longer self-sufficient — it's a first-class
citizen of a spec + test + code triple, not a replacement for code.

**Historical failure patterns repeat.** MDA, CASE tools, FIT/FitNesse — each failed not because
the idea was wrong but because:
1. The formality required for reliable generation exceeded what practitioners would write
2. The generated artifacts weren't better than what humans wrote directly
3. Maintenance of the spec became harder than maintaining the code

SDD with LLMs has a different character: specs can be informal (Markdown, natural language)
because LLMs handle ambiguity. But this creates the non-determinism problem that formal specs
avoided. The field is trading formality for accessibility, and paying for it in reliability.

**The API domain is the success case, not general SDD.** OpenAPI spec-first at Twilio, TypeSpec
at Microsoft, OpenAPI contract testing — these work because the spec domain (HTTP interface
contracts) is narrow, machine-parseable, and directly testable. General software spec-as-source
is much harder.

**Statistical evaluation is the honest answer to non-determinism.** Tessl's Harbor framework
is epistemically correct: you can't guarantee a spec produces correct code; you can measure
the probability distribution. This is a fundamentally different engineering guarantee than
type safety or formal proof — closer to property-based testing than unit testing.

**The "cheapness of code" counterargument is underrated.** The Marmelab/Scott Logic critique
is empirically backed. If code is now cheap (LLMs write it in seconds), the ROI on spec
maintenance must be very high to justify it. For most brownfield work, it isn't.

---

## Sources

- https://codespeak.dev/ — CodeSpeak homepage
- https://codespeak.dev/blog/codespeak-takeover-20260223 — Takeover blog post
- https://codespeak.dev/blog/greenfield-project-tutorial-20260209 — Greenfield tutorial
- https://codespeak.dev/blog/mixed-mode-tutorial-20260210 — Mixed mode tutorial
- https://www.piwheels.org/project/codespeak-cli/ — All 16 CLI versions
- https://pypi.org/project/codespeak-cli/ — PyPI package metadata
- https://github.com/codespeak-dev/hello-codespeak — Template repo
- https://tessl.io/blog/tessl-launches-spec-driven-framework-and-registry/ — Tessl launch
- https://tessl.io/blog/how-tessls-products-pioneer-spec-driven-development/ — Product details
- https://tessl.io/blog/taming-agents-with-specifications-what-the-experts-say/ — Expert quotes
- https://tessl.io/blog/how-to-evaluate-ai-agents-an-introduction-to-harbor/ — Harbor framework
- https://tessl.io/blog/cicd-for-context-in-agentic-coding-same-pipeline-different-rules/ — CI/CD
- https://tessl.io/blog/do-agent-skills-actually-help-a-controlled-experiment/ — Controlled exp
- https://tessl.io/blog/proposed-evaluation-framework-for-coding-agents/ — LangGraph eval
- https://tessl.io/blog/8-benchmarks-shaping-the-next-generation-of-ai-agents/ — Benchmarks
- https://tessl.io/blog/from-vibe-coding-to-spec-driven-development/ — SDD intro
- https://tessl.io/registry/tessl-labs/spec-driven-development/1.0.5 — Registry tile
- https://fortune.com/2024/11/14/tessl-funding-ai-software-development-platform/ — $750M
- https://docs.tessl.io/introduction-to-tessl/specifications — Spec format (404 at time of research)
- https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html — Kiro/spec-kit/Tessl comparison
- https://martinfowler.com/bliki/ModelDrivenArchitecture.html — MDA failure analysis
- https://blog.scottlogic.com/2025/11/26/putting-spec-kit-through-its-paces-radical-idea-or-reinvented-waterfall.html — 10x slower finding
- https://marmelab.com/blog/2025/11/12/spec-driven-development-waterfall-strikes-back.html — Waterfall critique
- https://arxiv.org/html/2411.13722 — TLA+ industrial practice review (IFM 2024)
- https://kiro.dev/docs/steering/ — Steering file format
- https://kiro.dev/docs/specs/ — Spec format
- https://kiro.dev/docs/specs/best-practices/ — Best practices
- https://kiro.dev/blog/introducing-kiro/ — Kiro introduction
- https://hedrange.com/2025/08/11/how-to-use-kiro-for-ai-assisted-spec-driven-development/ — Kiro deep dive
- https://devclass.com/2025/07/15/hands-on-with-kiro-the-aws-preview-of-an-agentic-ai-ide-driven-by-specifications/ — Hands-on
- https://docs.bmad-method.org/reference/workflow-map/ — BMAD workflow map
- https://redreamality.com/blog/-sddbmad-vs-spec-kit-vs-openspec-vs-promptx/ — Framework comparison
- https://github.com/github/spec-kit/discussions/152 — Evolving specs discussion
- https://arxiv.org/html/2602.00180v1 — SDD academic paper
- https://www.infoq.com/articles/spec-driven-development/ — Executable spec architecture
- https://addyosmani.com/blog/good-spec/ — Spec quality (2,500 agent config file analysis)
- https://nordicapis.com/understanding-the-root-causes-of-api-drift/ — 75% non-conformance stat
- https://www.kinde.com/learn/ai-for-software-engineering/ai-devops/spec-drift-the-hidden-problem-ai-can-help-fix/ — Spec drift
- https://apievangelist.com/2024/02/08/stripes-monolithic-openapi-vs-twilio-modular-openapis/ — Stripe/Twilio OpenAPI
- https://alistairmavin.com/ears/ — EARS notation
- https://www.softwareseni.com/rolling-out-spec-driven-development-the-team-adoption-and-change-management-playbook/ — Adoption playbook
- https://www.marvinzhang.dev/blog/sdd-tools-practices — Tool comparison with adoption signals
- https://zarar.dev/spec-driven-development-from-vibe-coding-to-structured-development/ — OpenSpec workflow
- https://www.thoughtworks.com/en-us/insights/blog/agile-engineering-practices/spec-driven-development-unpacking-2025-new-engineering-practices — Thoughtworks analysis
- https://fitnesse.org/FrontPage — FitNesse current status
- https://developers.redhat.com/articles/2025/10/22/how-spec-driven-development-improves-ai-coding-quality — Red Hat
- https://learn.microsoft.com/en-us/azure/developer/typespec/overview — TypeSpec
- https://arxiv.org/pdf/2505.13938 — CLEVER benchmark (LLM + formal verification)
