---
title: "Specpunk Buyer and Procurement Memo"
date: 2026-03-12
status: memo
origin: follow-up after gap mapping, tool capability matrix, and compliance operating model
scope: local corpus only
---

# Specpunk Buyer and Procurement Memo

## Verdict

The most defensible first wedge is **not** "AI coding for everyone."

The most defensible first wedge is:

- teams already using agentic coding tools
- in codebases large enough for review and coordination pain to dominate
- with either compliance pressure or visible review-trust breakdown

That points to a first commercial position closer to:

- `reliability / review-control infrastructure`
- with a `platform / DevEx / VP Eng` buyer path

and farther from:

- developer-seat upsell on top of autocomplete
- solo/small-team productivity tooling

## Scope Note

This memo is based on the local research corpus only.

Important:

- `Corpus-supported` means there is direct support in the existing research files.
- `Hypothesis` means an inferred GTM conclusion built from those signals.
- This is **not** customer-validated discovery.

## What The Corpus Already Supports

### 1. Economics flip only for certain team/task shapes

Direct support:

- SDD-like overhead is strongly negative for bug fixes and small features, roughly neutral for medium features, and positive for large, multi-service, and regulated work (`2026-03-12-delve-sdd-slowdown.md:236-243`).
- The recommendation becomes more favorable as team size grows; `1-3` developers are a bad fit, while `10-25` developers can justify phased adoption with positive ROI in `3-6` months (`2026-03-12-delve-sdd-slowdown.md:248-257`).
- The economic case flips most clearly in regulated environments, large-team coordination, and agent-heavy execution contexts (`2026-03-12-delve-sdd-slowdown.md:875-883`).

### 2. Rollout is organizational, not just individual

Direct support:

- enterprise adoption friction clusters around `Jira/ADO integration`, `multi-repo complexity`, `cross-functional participation`, and workflow lock-in (`2026-03-12-deep-spec-driven-dev.md:720-737`)
- practitioner rollout guidance assumes:
  - 90-day phased adoption
  - training investment
  - a champion network
  - support FTE during rollout
  (`2026-03-12-deep-spec-driven-dev.md:739-756`)

This implies the adoption owner is rarely just an individual engineer.

### 3. Review economics already justify adjacent budget

Direct support:

- AI-assisted review tools show meaningful review-time and cost reduction in the corpus
- adjacent tools are already sold against manual review cost and review latency
- break-even can happen quickly when the review problem is acute (`2026-03-12-deep-code-review.md:539-558`)

This means there is already an established budget narrative for "developer workflow tools that reduce review cost."

### 4. Mandatory-infrastructure motion is plausible under reliability or compliance pressure

Direct support:

- Podjarny/Snyk is used in the corpus as the structural analog: security became budgeted not because developers wanted it, but because ignoring it became organizationally impossible (`2026-03-12-delve-sdd-slowdown.md:340-342`)
- the same file argues that adoption growth is best explained by long-horizon reliability pressure, regulatory forcing functions, and infrastructure-platform logic (`2026-03-12-delve-sdd-slowdown.md:613-622`)

This does not prove Specpunk becomes mandatory infrastructure, but it does support that the winning motion is likely `risk reduction`, not `developer delight`.

## First ICP

## Corpus-supported constraints

Bad first ICP:

- solo developers
- teams under 5 developers
- greenfield vibe-coding
- orgs without meaningful review pain

Why:

- the corpus repeatedly shows overhead dominates for small teams and small tasks (`2026-03-12-delve-sdd-slowdown.md:236-257`, `2026-03-12-delve-sdd-slowdown.md:873-883`)

Good first-ICP characteristics:

- 10+ engineers in a shared codebase
- visible AI adoption already underway
- brownfield work, not toy greenfield
- repeated review/trust friction
- either compliance sensitivity or multi-team coordination pressure

## GTM hypothesis

The best first ICP is:

**AI-heavy product/platform engineering teams in 10-25+ developer orgs, already using Claude Code / Cursor / Codex-style tools, where review throughput and trust have become the bottleneck.**

Best sub-segments:

1. `Platform / DevEx-led teams` standardizing AI-assisted development across multiple engineers
2. `Compliance-sensitive product teams` where auditability and traceability are already required
3. `AI-heavy backend teams` with medium-to-large brownfield changes and recurring reviewer uncertainty

Less likely first ICP:

- pure AppSec as the primary wedge

Reason:

- the current thesis is broader than security, and the strongest immediate value in the corpus is around review containment, intent continuity, and behavior evidence, not just vulnerability detection

## Buyer Map

This section is a GTM hypothesis derived from the corpus.

| Role | Likely fit | Why |
|---|---|---|
| `VP Eng / Head of Engineering` | `Economic buyer` in larger orgs | Owns delivery risk, review capacity, and AI rollout outcomes |
| `Head of Platform / DevEx` | `Economic buyer` or strong co-buyer | Owns engineering workflow standardization, CI/PR controls, and cross-team rollout |
| `Engineering Manager` | `Champion` for first pilot | Feels review bottleneck and context-drift pain most directly |
| `Senior IC / Staff engineer` | `Power user` and design partner | Owns module boundaries, review quality, and practical workflow fit |
| `AppSec / compliance lead` | `Secondary buyer` or `blocker-turned-sponsor` | Becomes central when auditability, traceability, or regulated workflows dominate |
| `Individual developer` | `User`, rarely buyer | Feels pain but usually does not own platform/process budget |

## Budget Map

The corpus does not prove a single budget line, so this is an explicit hypothesis.

Most likely budget adjacency:

1. `Developer productivity / DevEx tooling`
2. `AI review / code quality tooling`
3. `Platform engineering` budget
4. `Compliance / governance` budget in regulated contexts

Least likely early budget:

- pure `AI coding seat` budget

Why:

- the product is not promising more generation throughput
- it is promising safer rollout, better reviewability, and more reliable coordination
- those claims map better to quality/process budgets than to raw coding-assistant seat expansion

## Trigger Events

The corpus supports these pressure patterns. The exact order is a hypothesis.

Most credible trigger events:

1. `Review backlog or SLA collapse`
   Why:
   review economics and trust-gap evidence are already strong in the corpus (`2026-03-12-deep-code-review.md:539-560`)

2. `AI rollout beyond individual experimentation`
   Why:
   once 10+ people use agents in the same codebase, coordination and policy surfaces matter much more (`2026-03-12-delve-sdd-slowdown.md:248-257`, `2026-03-12-deep-spec-driven-dev.md:739-756`)

3. `Incident spike or defect escape tied to AI-assisted changes`
   Why:
   this matches the `mandatory infrastructure` adoption logic in the Snyk analogy (`2026-03-12-delve-sdd-slowdown.md:340-342`)

4. `Regulated rollout or audit finding`
   Why:
   regulated contexts are where documentation overhead stops looking like overhead (`2026-03-12-delve-sdd-slowdown.md:242-243`, `2026-03-12-delve-sdd-slowdown.md:881-883`)

5. `Platform standardization initiative`
   Why:
   enterprise adoption friction is explicitly about systems, workflow, and integration, not just model quality (`2026-03-12-deep-spec-driven-dev.md:720-737`)

## Land Motion

### Recommended first motion

Start as a `team workflow control layer`, not an enterprise-wide spec program.

Practical land motion:

1. One team
2. One bounded module set
3. One review-heavy workflow
4. One AI execution substrate already in active use

Why:

- the corpus shows rollout friction and training cost are real
- broad up-front mandate is likely to trigger the same resistance patterns SDD already hits

### Recommended expansion motion

Expand from:

- `pilot team` -> `shared platform pattern` -> `org policy for AI-assisted changes`

Not:

- `company-wide spec mandate on day one`

### Best initial use case

The strongest initial use case is not "write specs for everything."

It is:

- medium-sized brownfield changes
- where reviewers lack confidence
- where teams want bounded scope and behavior evidence

That lines up with the current thesis much better than feature-spec authoring as the initial sale.

## Packaging Hypothesis

This section is a hypothesis anchored to adjacent pricing signals in the corpus.

Known local anchors:

- SDD/tooling examples in the corpus reference roughly `$20-40/dev/month` licensing levels in rollout estimates (`2026-03-12-deep-spec-driven-dev.md:748-756`)
- AI review tools in the corpus are modeled around low-per-seat monthly cost with ROI from review savings (`2026-03-12-deep-code-review.md:550-556`)

Most plausible early packaging:

1. `Team/workspace plan`
   Best when the sale is about workflow standardization and review controls.

2. `Per-active-engineer pricing in low tens of dollars per month`
   Plausible only as an anchor, not yet validated WTP.

3. `Enterprise add-on for governance/compliance mode`
   Makes sense once team-shared or enterprise-managed transcript policies exist.

Least compelling early packaging:

- usage-based pricing by token or extraction event

Why:

- buyers here care about risk control and workflow predictability
- token-linked pricing makes the product feel like a model accessory instead of workflow infrastructure

## Likely Objections

The corpus already predicts these objections:

1. `This adds process overhead`
2. `Specs take longer than coding`
3. `It breaks my flow`
4. `We already have AI review tooling`
5. `We cannot rework Jira/ADO and repo workflows around this`

These are not edge cases. They are central adoption blockers (`2026-03-12-deep-spec-driven-dev.md:720-737`, `2026-03-12-delve-sdd-slowdown.md:370-382`).

## Positioning Implication

The cleanest positioning is:

**Specpunk reduces review risk and change sprawl in AI-assisted brownfield development.**

This is better than:

- "program in natural language"
- "specs for everyone"
- "replace your coding agent"

It is also more aligned with the evidence already present in the corpus.

## What Must Be Customer-Validated Next

The current corpus is not enough to settle these:

1. Is the first buyer actually `platform/DevEx`, or does the first budget open under `EM/VP Eng`?
2. Does `compliance/audit` open budget faster than `review bottleneck`, or vice versa?
3. Is the first pilot sold on `scope control`, `review bundle`, or `intent continuity`?
4. Does the market prefer `team workflow infrastructure` packaging or per-seat pricing?
5. Does AppSec become a real wedge only after the governance mode exists?

## Recommended Next Discovery Pack

1. 8-12 interviews with teams already deep on `Claude Code`, `Cursor`, or `Codex`
2. Segment questions by pain:
   - review backlog
   - incident escape
   - audit/compliance
   - multi-agent coordination
3. Ask directly:
   - who would own budget
   - which budget line it comes from
   - what event would make this urgent
   - what rollout shape would be politically viable

## Bottom Line

The current research base supports a fairly clear commercial direction:

- do **not** target solo/small teams first
- do **not** sell "more generation"
- do sell `review/risk/control infrastructure` for AI-heavy brownfield teams

Most likely first commercial path:

- `platform / DevEx / VP Eng` buyer path
- triggered by review-trust breakdown, AI rollout standardization, or compliance pressure
- landed on a single team before expanding into shared workflow infrastructure
