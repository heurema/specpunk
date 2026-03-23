# Decisions

Last updated: 2026-03-14
Owner: Vitaly
Status: active

## 2026-03-12 / D-001

Decision:
- `brief.md` is the product source of truth

Reason:
- current-cycle execution will drift unless one document wins on conflict

Consequence:
- any strategic change must land here first, then update `brief.md`

## 2026-03-12 / D-002

Decision:
- the public surface stays a static object with a minimal prompt-based CTA for now

Reason:
- a live contact channel creates response-time obligations before the product loop exists

Consequence:
- the site should stay useful without pretending to have a full intake flow

## 2026-03-12 / D-003

Decision:
- the first wedge is `scope enforcement + minimal review artifact`

Reason:
- scope alone is too invisible
- evidence alone is too abstract
- together they create the first believable review improvement

Consequence:
- the first product proof must show declared boundary versus actual change plus a short review note

## 2026-03-12 / D-004

Decision:
- product docs must be updated in the same diff as any meaningful product change

Reason:
- otherwise documents lag reality and stop being operational

Consequence:
- if a doc cannot be updated in the same diff, it must get a visible stale note with owner and due date

## 2026-03-12 / D-005

Decision:
- `Go` is the primary runtime for the future Specpunk CLI

Reason:
- the product should ship as a single binary devtool
- `Go` gives the simplest path to a portable CLI without runtime setup friction
- `Rust` remains a valid long-term option, but is unnecessary complexity at the current stage

Consequence:
- new CLI foundation work should land in `Go`
- the current Python tool is treated as a spike, not as the long-term runtime base

## 2026-03-14 / D-006

Decision:
- pause implementation after the current wedge proof and switch the next pass to idea research

Reason:
- the repo now has a credible wedge proof and product path
- the next major risk is thesis drift, not missing CLI mechanics
- continuing implementation before another research pass would increase local polish faster than product certainty

Consequence:
- no new product-path code should be treated as the default next move
- the next return to execution starts from `pause-handoff-2026-03-14.md`, `queued-next-tasks.md`, and `open-questions.md`
- if research changes the thesis, update `brief.md` before resuming implementation
