# Roadmap

Last updated: 2026-03-14
Owner: Vitaly
Status: **archived** (superseded by [ROADMAP-v2.md](ROADMAP-v2.md) on 2026-03-27)

## Roadmap Rule

This file tracks milestone outcomes, not a task backlog.

A milestone is complete only when it has:

- a working artifact
- a dogfooded usage path in this repo
- a short evidence note showing why it matters

Execution note:
- implementation is paused for idea research
- milestone definitions stay current until a decision changes them

## Milestone 0

Status: complete
Outcome:
- public surface is live and honest enough to show the product shape
- the repo has a product operating system under `docs/product/`

Exit signal:
- `specpunk.com` serves the current public surface
- product docs are the active planning layer for this repo

## Milestone 1

Status: active
Outcome:
- one task in this repo can declare scope
- the actual change can be checked against that scope
- a minimal review artifact explains the difference between declared and actual change
- the flow exists as a repeatable task-directory workflow instead of as hand-wired file paths

Exit signal:
- one real repo-local code change runs through `specpunk task init` or an equivalent task-directory path
- one dogfooded task produces an understandable `review` artifact
- the artifact is more useful than the raw diff alone

## Milestone 2

Status: queued
Outcome:
- the minimal artifact pack exists as a repeatable shape:
  `intent`, `scope`, `evidence`, `review`

Exit signal:
- the same artifact shape works on at least 3 repo tasks
- artifact size stays compact

## Milestone 3

Status: queued
Outcome:
- terminology and invariant checks catch meaningful drift

Exit signal:
- at least one real contradiction or terminology mismatch is caught on this repo or a benchmark repo
- false positives stay low enough to remain usable

## Milestone 4

Status: queued
Outcome:
- the product is tested against external repositories and conversations

Exit signal:
- 3 to 5 external conversations are logged with the interview template
- at least one external repo task validates the wedge beyond this repo

## Not On This Roadmap Yet

- full transcript mining
- multi-tool live integrations
- benchmark automation at scale
- rich dashboards
- enterprise workflow depth
