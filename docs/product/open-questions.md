# Open Questions

Last updated: 2026-03-14
Owner: Vitaly
Status: paused pending research

## Rule

These questions gate the next implementation pass.
If research changes the thesis, update `decisions.md` and `brief.md` before writing new product code.

## Q-001

Question:
- how manual should the first artifact pack be before automation starts helping more than it hurts?

Current hypothesis:
- manual or semi-manual is fine for the first wedge as long as the artifact stays compact

Next trigger:
- revisit after the first real repo-local code change through `task-dir`

## Q-002

Question:
- what is the smallest evidence artifact that creates a real reviewer aha moment?

Current hypothesis:
- a short review note tied to declared scope is enough for the first proof

Next trigger:
- revisit after the first real repo-local code change and the first external conversation

## Q-003

Question:
- how much extra value comes from session-aware extraction over the portable core?

Current hypothesis:
- useful later, but not required for the first wedge

Next trigger:
- revisit only after the portable core works on one real repo-local change, not only on demos and external proofs

## Q-004

Question:
- when should the public surface gain a real contact path?

Current hypothesis:
- only after the static surface is live and the team can respond consistently

Next trigger:
- revisit after the first 3 to 5 conversations are logged

## Q-005

Question:
- what exact moment makes Specpunk urgent enough for a real team to adopt: scope drift, review clarity, or behavior proof?

Current hypothesis:
- the strongest initial pull is not "AI writes code" but "reviewers cannot safely reason about AI-assisted change"

Next trigger:
- revisit after 3 focused product conversations with engineers, reviewers, or engineering managers

## Q-006

Question:
- what is the smallest task-truth source we should prioritize in v1: manual task directory, issue text, PR text, or session transcript?

Current hypothesis:
- the v1 core should stay repo-native and work with manual task directories first, while issue and PR text remain validation inputs rather than the primary runtime path

Next trigger:
- revisit after the next research pass on adjacent tools and task-truth sources

## Q-007

Question:
- what minimum evidence artifact creates trust beyond simple file-boundary checks?

Current hypothesis:
- a short review note tied to declared scope may be enough for the first wedge, but behavior-oriented evidence is the likely next step

Next trigger:
- revisit before expanding the artifact set beyond `intent`, `scope`, and `review`

## Q-008

Question:
- what public promise can the site make truthfully right now without implying a fuller product than exists?

Current hypothesis:
- the public surface can truthfully promise a review boundary and compact artifact shape, but should not imply full automation or team workflow depth yet

Next trigger:
- revisit before changing the public copy or adding any stronger CTA
