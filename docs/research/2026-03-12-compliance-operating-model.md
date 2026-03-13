---
title: "Specpunk Compliance Operating Model"
date: 2026-03-12
status: memo
origin: follow-up after gap mapping and tool capability matrix
scope: local corpus only
---

# Specpunk Compliance Operating Model

## Verdict

If Specpunk supports `transcript-derived intent`, the product needs an explicit compliance operating model from day one.

The safest product stance is:

- `local-first` by default
- `derived artifacts over raw transcripts`
- `human-owned classification and release decisions`
- `tool-specific ingestion rules`, not one universal transcript policy

The core reason is simple:

- raw session data may contain secrets, PII, proprietary business logic, and unpatched security details
- extracting it creates a second sensitive corpus
- current legal responsibility still sits primarily with the shipping organization, not the model vendor

## What This Memo Is

This is a proposed operating model for:

- data handling
- retention
- access control
- deployment modes
- escalation paths
- tool-specific restrictions

This is **not** a legal opinion and **not** a claim that the product is certification-ready for safety-critical environments.

## Grounded Facts From the Corpus

The current local research base already supports these facts:

1. Thinking blocks can contain secrets, PII, business logic, and security vulnerabilities (`2026-03-12-delve-thinking-blocks.md:867-877`).
2. Transcript extraction creates a second sensitive corpus and enables cross-session leakage if not isolated (`2026-03-12-delve-thinking-blocks.md:878-886`).
3. The corpus already recommends local-only handling, encryption by default, fail-closed secret detection, and no cloud upload without explicit consent (`2026-03-12-delve-thinking-blocks.md:920-924`, `2026-03-12-delve-thinking-blocks.md:1040-1044`).
4. Prompt/session provenance matters for auditability and can be modeled via `W3C PROV` (`2026-03-12-deep-intent-preservation.md:218-233`).
5. Append-only transcript trails are structurally similar to audit trails, but they still need a semantic and governance layer (`2026-03-12-deep-intent-preservation.md:388-406`).
6. Classification of sensitive content cannot be fully delegated to an LLM; some classes still require legal/engineering judgment (`2026-03-12-deep-code-review.md:413-417`).
7. Liability for shipped AI-generated code currently remains primarily with the developer and organization that ship it (`2026-03-12-deep-nextgen-langs.md:497-510`).

## Operating Principles

### 1. Raw transcripts are toxic by default

Treat raw session data as high-risk operational material, not as ordinary product telemetry.

Implication:

- do not treat transcript collection as a harmless analytics feature
- do not sync raw transcripts by default
- do not assume extracted reasoning is safe because it is "just text"

### 2. Derived intent is preferred over stored reasoning

The durable object should be a compact, sanitized, human-reviewable artifact such as:

- `intent.md`
- `glossary.md`
- `invariants.md`
- `review.md`
- a structured decision record

Not the raw thinking block itself.

### 3. Classification stays human-owned

The system may assist classification and redaction, but it must not claim to certify:

- PII handling
- regulated data classes
- BAA-relevant content
- organization-specific restricted information

### 4. Tool asymmetry is part of compliance

Different tools produce different compliance risk:

- `Claude Code`: highest extraction value, highest raw-data risk
- `Codex CLI`: lower reasoning leakage risk because reasoning is encrypted, but plaintext messages/tool calls still matter
- `Gemini CLI`: lower reasoning capture surface, but still contains visible prompts/responses
- `Cursor`: partial access with schema fragility, which is itself a governance risk

## Data Classes

Recommended classes:

### Class A: Raw vendor session

Examples:

- Claude JSONL session
- Codex JSONL session
- Gemini session JSON
- Cursor workspace/session storage

Policy:

- default storage: vendor-native location only
- default sharing: `No`
- default retention in Specpunk: `Do not import by default`
- exception: explicit local import for opted-in workflows

### Class B: Raw reasoning block

Examples:

- Claude thinking block
- any other vendor-native reasoning object if accessible

Policy:

- default storage: local ephemeral only
- default sharing: `No`
- default retention: shortest possible window
- if secret hit or sensitive-domain hit: `drop, do not persist`

Important:

For Claude-style signed thinking blocks, the original must remain immutable and separate. If sanitization is needed, store the sanitized derivative separately because modifying the original can break session resumability (`2026-03-12-delve-thinking-blocks.md:748-753`).

### Class C: Extracted decision candidate

Examples:

- draft rationale summary
- candidate tradeoff extraction
- candidate constraints list

Policy:

- default sharing: local or restricted team scope only
- default retention: moderate
- must be marked as `machine-derived`
- must not be treated as authoritative until reviewed

### Class D: Sanitized approved intent artifact

Examples:

- reviewed `intent.md`
- reviewed `glossary.md`
- reviewed `invariants.md`
- approved AgDR/ADR-style record

Policy:

- shareable in repo
- can be retained long-term
- becomes the durable system of record

### Class E: Evidence and review artifact

Examples:

- `review.md`
- `evidence.md`
- behavior summary
- scope/invariant check output

Policy:

- shareable if it contains no prohibited raw content
- may live in repo or CI artifacts
- should reference provenance, not embed high-risk raw transcript segments

### Class F: Provenance metadata

Examples:

- session id
- tool name
- model/version
- timestamp
- artifact derivation chain

Policy:

- retain longer than raw text
- safe default for team-shared and enterprise modes
- use for auditability even when raw transcript retention is disabled

## Recommended Retention Policy

This section is a proposed policy, not a sourced fact.

| Data class | Local-only default | Team-shared default | Enterprise-managed default |
|---|---|---|---|
| Class A raw session | leave in vendor store; no Specpunk copy by default | not shared by default | import only by policy exception |
| Class B raw reasoning | ephemeral or very short retention | no shared retention | disabled by default |
| Class C extracted decision candidate | short-to-moderate retention | restricted project scope | controlled retention with owner |
| Class D approved intent artifact | long-lived | long-lived | long-lived |
| Class E evidence/review | moderate or repo-lifetime if sanitized | moderate | per audit policy |
| Class F provenance metadata | moderate-to-long | long | long |

Practical interpretation:

- raw text should age out first
- sanitized, approved artifacts should live the longest
- provenance metadata should outlive raw reasoning

## Deployment Modes

### Mode 1: Local-only

This should be the default.

Characteristics:

- no server component required
- no cloud sync
- per-developer local database/files
- explicit opt-in for importing session material

Allowed:

- code-based draft generation
- local transcript extraction
- local review bundle generation

Best for:

- early product
- small teams
- privacy-sensitive pilots

### Mode 2: Team-shared

This should be opt-in and policy-gated.

Characteristics:

- shared storage for approved artifacts and limited metadata
- raw transcript sharing disabled by default
- access limited by project/team boundary

Allowed:

- sanitized approved intent artifacts
- provenance metadata
- limited extracted candidates if project policy allows

Requires:

- project isolation
- role-based access
- retention enforcement
- deletion workflow

### Mode 3: Enterprise-managed

This should be a later mode, not the default architecture.

Characteristics:

- centralized policy engine
- retention schedules
- access logs
- explicit admin controls
- export and delete workflows

Requires:

- data-class-aware storage
- auditable chain of custody
- tenant/project isolation
- incident handling process
- legal/security review before enabling any raw-transcript feature

## Access Model

Recommended access rules:

1. Raw transcript and raw reasoning are creator-local by default.
2. Sanitized approved artifacts are project-readable.
3. Provenance metadata is readable to reviewers and project owners.
4. Cross-project queries are disabled by default.
5. Admin access to raw stores should be exceptional and auditable.

## Mandatory Controls

### Secret scanning on ingestion

Must run before any persistence outside the vendor-native file.

If secret detection hits:

- do not store the raw extracted block
- emit an event
- require manual review before any sanitized derivative is kept

### Project isolation

The model must assume that a single session can reference multiple repositories or directories.

Therefore:

- store project identity explicitly
- block cross-project retrieval by default
- prevent a decision extracted from project A from surfacing in project B without explicit linkage

### Immutable source, mutable derivative

For sources with resumability/signature semantics, keep:

- immutable original
- separately stored sanitized derivative

Never rewrite the source transcript in place.

### Provenance without over-retention

Keep enough metadata to answer:

- what artifact came from which session
- who approved it
- which model/tool produced it

But do not require indefinite retention of raw reasoning text.

### Human approval for release to repo

Only Class D artifacts should become durable repo truth.

Machine-derived candidates stay candidates until reviewed.

## Tool-Specific Handling Rules

### Claude Code

Policy:

- allow first-class local extraction
- support hook-based real-time capture
- treat as highest-value / highest-risk substrate

Special rules:

- original transcript remains untouched
- sanitized derivatives stored separately
- raw reasoning sharing disabled by default

### Codex CLI

Policy:

- do not design compliance flow around raw reasoning extraction
- use plaintext agent messages, tool calls, and repo changes where needed
- prioritize code-derived and outcome-derived artifacts

Why:

- reasoning is present but not user-accessible in usable form (`2026-03-12-delve-thinking-blocks.md:952-973`)

### Gemini CLI

Policy:

- treat transcript import as visible conversation import, not reasoning import
- support only baseline session-aware features unless stronger substrate evidence appears

Why:

- the corpus shows final response storage but no separate reasoning blocks (`2026-03-12-delve-thinking-blocks.md:975-999`)

### Cursor

Policy:

- pilot only until schema stability is better understood
- avoid promises that depend on undocumented local schema

Why:

- partial accessible data exists, but extraction is harder and version-fragile (`2026-03-12-delve-thinking-blocks.md:1001-1012`)

## Escalation Rules

Escalate to human security/legal review if any of these occur:

- secret detector hit
- likely PII in extracted material
- auth, payments, cryptography, healthcare, or regulated domain code
- request to move from local-only to team-shared raw transcript handling
- request to upload transcript-derived material to third-party cloud systems
- request to use transcript-derived artifacts in external compliance evidence

## Audit and Defensibility

If the product claims auditability, it should store at least:

- artifact id
- source class
- source session id or vendor-local locator
- tool and model/version when known
- extraction timestamp
- reviewer/approver
- derivation link to sanitized artifact

This is the minimum defensible chain.

`W3C PROV` is a reasonable schema direction, but the immediate need is not standards theater. The immediate need is a consistent internal provenance model with stable identifiers.

## Non-Goals

This operating model does **not** assume:

- automatic legal classification
- cloud-first telemetry ingestion
- raw transcript retention as a product default
- certification-grade evidence for safety-critical environments
- one universal policy that fits Claude, Codex, Cursor, and Gemini equally

## Recommended Product Position

The cleanest position is:

- default product: repo-native control layer
- optional feature: local transcript-derived drafting
- advanced feature: governed team-shared intent service

Not:

- always-on transcript mining
- always-on cloud sync
- vendor-neutral reasoning extraction parity

## Implementation Order

1. Ship local-only mode first.
2. Support only sanitized approved artifacts in repo.
3. Add provenance metadata before adding shared raw stores.
4. Add team-shared mode only with retention, deletion, and access controls.
5. Treat enterprise-managed mode as a separate operating profile, not a flag.

## Bottom Line

`Automatic draft extraction` is a strong adoption wedge, but it is also the highest governance risk in the product.

So the operating model should be:

- **local-first**
- **sanitized-by-default**
- **raw transcripts exceptional**
- **human approval before durable sharing**
- **tool-specific ingestion rules instead of fake parity**
