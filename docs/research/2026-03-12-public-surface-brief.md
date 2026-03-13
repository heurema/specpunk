---
title: "Specpunk Public Surface Brief"
date: 2026-03-12
status: draft
origin: UI translation of the public surface concepts report
---

# Specpunk Public Surface Brief

## Purpose

This document turns the earlier `public surface concepts` memo into a concrete UI shape.

It is **not** a conventional landing page brief.

It is a brief for a `public surface` that should:

1. make the right person stop
2. make them dig deeper
3. show the product as a real object
4. avoid startup theater

## Design Thesis

The public surface should feel closer to:

- an `incident console`
- a `research notebook`
- a `review control panel`

and farther from:

- a polished SaaS homepage
- a pitch deck in HTML
- a generic "AI for teams" site

The UI should say:

`this is a system for containing AI-assisted change`

before it says:

`please trust us`

## Recommended Shape

Use a three-layer page:

1. `Boundary Demonstration`
2. `Public Lab Notebook`
3. `Control Room`

The page should scroll, but the first screen must already communicate the product.

## Screen 1

### What the first screen needs to do

The first screen should show one concrete conflict:

`this task should touch 3 files`

versus:

`the agent touched 11`

Then immediately show the controlled version:

- declared scope
- intent card
- evidence
- review posture

That is the product.

No generic hero paragraph is required.

## Desktop Wireframe

```text
+--------------------------------------------------------------------------------------------------+
| SPECPUNK                                   notebook   artifact pack   conversation               |
+--------------------------------------------------------------------------------------------------+
| This change should touch 3 files. It touched 11.                                                 |
|                                                                                                  |
| WITHOUT CONTROL                         CHANGE 001                         WITH SPECPUNK          |
|                                                                                                  |
| [raw diff stream]                       [control rail]                     [review artifact]      |
|                                                                                                  |
| src/auth/login.ts                       task: add session timeout          intent                 |
| src/auth/session.ts                     allowed scope: 3 files             - add timeout rule     |
| src/billing/subscription.ts   !         actual changes: 11 files           - preserve refresh     |
| src/notifications/email.ts    !         out of scope: 8                    - no billing impact    |
| src/ui/header.tsx              !        tests: 3 passed / 2 missing        glossary/invariants    |
| ...                                      confidence: unstable              - "session" != token   |
|                                                                                                  |
| risk markers:                            [raw] [scoped] [evidence]         evidence               |
| - cross-boundary edits                                                     - test delta           |
| - undefined intent                                                       - behavior summary      |
| - review confidence: low                                                   - reviewer posture     |
|                                                                                                  |
|                                                                                                  |
+--------------------------------------------------------------------------------------------------+
| latest notebook delta              benchmark note            open question           next build   |
+--------------------------------------------------------------------------------------------------+
| control room index: intent | glossary | invariants | scope | evidence | review                  |
+--------------------------------------------------------------------------------------------------+
```

## How to read the first screen

### Left column

This is the bad state.

It should look noisy, overreaching, slightly uncomfortable:

- too many touched files
- warning markers
- unclear intent
- review anxiety

### Center rail

This is the explanation spine.

It should contain only a few hard facts:

- task name
- expected scope
- actual scope
- missing evidence

This rail is the semantic bridge between the two sides.

### Right column

This is the controlled state.

It should look calmer and denser:

- compact `intent card`
- explicit `scope`
- behavior/test evidence
- review recommendation

Not "green success UI". More like:

`contained enough to reason about`

## Mobile Shape

On mobile the same idea becomes a stacked incident narrative:

```text
+--------------------------------------+
| SPECPUNK                             |
| This change should touch 3 files.    |
| It touched 11.                       |
+--------------------------------------+
| WITHOUT CONTROL                      |
| 11 files changed                     |
| 8 out of scope                       |
| review confidence: low               |
+--------------------------------------+
| CONTROL RAIL                         |
| task: add session timeout            |
| allowed scope: 3 files               |
| missing evidence: 2 checks           |
+--------------------------------------+
| WITH SPECPUNK                        |
| intent card                          |
| scope ok                             |
| evidence attached                    |
| review posture: contain + inspect    |
+--------------------------------------+
| notebook                             |
| latest delta                         |
+--------------------------------------+
| control room                         |
+--------------------------------------+
```

The mobile version should still feel like one coherent object, not a shrunk marketing site.

## Scroll Model

### Layer 1: Boundary Demonstration

This occupies the first `70-90vh`.

It should be immediately understandable without reading paragraphs.

Possible microcopy:

- `This task should touch 3 files. It touched 11.`
- `AI changes are easy to make. Hard to contain.`
- `Review breaks when intent breaks.`

The first line should be factual, not aspirational.

### Layer 2: Public Lab Notebook

After the main contrast, the page drops into a living notebook.

This section should look like a running build log, not a blog homepage.

Recommended modules:

- `thesis delta`
- `latest benchmark learning`
- `current unknowns`
- `what changed this week`
- `why this is still unresolved`

Each entry should be short and decision-rich.

Example card titles:

- `Why scope enforcement comes before extraction`
- `What broke in the last benchmark pass`
- `What we still cannot prove about review accuracy`

### Layer 3: Control Room

This is the deepest layer.

It should expose the product's artifacts more directly:

- `intent.md`
- `glossary.md`
- `invariants.md`
- `scope.yml`
- `evidence.md`
- `review.md`

This should feel closer to opening a live module than reading marketing copy.

## Visual Language

### General feel

Do not make it glossy.

Do not make it dark-by-default unless there is a strong reason.

Use a surface that feels:

- technical
- paper-like
- slightly archival
- alert, but not loud

### Color direction

Suggested palette direction:

- base background: warm off-white or pale stone
- primary text: graphite
- risk accent: rust / signal red
- contained state accent: dark green or deep teal
- notebook metadata: muted brown-gray

The page should not look cyberpunk or terminal-larp.

### Typography

Use contrast between:

- one human, editorial face for sparse headings
- one mono face for artifacts, labels, and system state

The serif should not dominate.

The mono should carry trust.

### Motion

Motion should reveal state change, not decorate.

Useful motions:

- scrub between `without control` and `with Specpunk`
- expand `why flagged` details inline
- notebook cards slide in as chronological entries
- control-room drawers open with no flourish

Avoid generic SaaS fade/float animation.

## Navigation

Navigation should be minimal.

Recommended top bar:

- `SPECPUNK`
- `notebook`
- `artifact pack`
- `conversation`

Not:

- `features`
- `pricing`
- `customers`
- `book demo`

At least for the first public version.

## Interaction Patterns

The page should not force a fake CTA block.

Use one or two real interactions:

### `Open the artifact pack`

This should open a compact object:

- sample `intent.md`
- sample `scope.yml`
- sample `review.md`

This is a better action than "join waitlist".

### `Describe your last scary AI PR`

This can be a simple, raw input surface.

Not a lead-gen form.

More like:

`What was the last AI-generated change you did not trust?`

### `Request a conversation`

If present, keep it plain.

Do not call it a demo.

Do not wrap it in enterprise funnel language.

## What should be absent

The first version should probably omit:

- testimonials
- logo clouds
- vanity metrics
- investor-flavored certainty
- generic product feature grids
- polished fake screenshots
- broad "for teams that..." taxonomy copy

## Concrete UI recommendation

If only one direction should be built first, build this:

1. a full-screen `Boundary Demonstration`
2. a short `Notebook` strip with 3-5 live entries
3. a compact `Artifact Pack` drawer
4. a plain `conversation` action

That is enough for a first public surface.

## What this should feel like

Not:

- "here is a startup"
- "here is an AI tool"
- "here is a polished future"

Closer to:

- "here is a real problem, shown clearly"
- "here is one way to contain it"
- "here is the thinking behind it"
- "here is the object being built"

## Open decisions

These still need to be chosen before implementation:

1. Which exact scenario powers the first-screen contrast?
2. Should the first screen start with one sentence or with almost no text?
3. Should `conversation` mean email, issue, form, or something else?
4. Should the notebook live on the same page or as a linked deeper layer?

## Immediate next step

Before writing code, define one concrete `Boundary Demonstration` scenario in detail:

- task
- expected scope
- out-of-scope edits
- intent card
- evidence card
- review posture

Once that scenario exists, the UI can be implemented with much less ambiguity.
