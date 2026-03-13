# Generated Review Artifact

Task: Review a drifted git-backed change inside the sandbox repo where research notes or deploy config were also touched.
Decision: inspect
Reason: blocked files were touched

## Scope Summary

- Declared allowed patterns: 2
- Declared blocked patterns: 2
- Changed files: 3
- In scope: 1
- Out of scope: 2
- Blocked touched: 2
- Scope status: drifted

## Allowed Patterns

- `site/index.html`
- `site/style.css`

## Blocked Patterns

- `wrangler.toml`
- `docs/research/**`

## Changed Files

- `docs/research/notes.md`
- `site/index.html`
- `wrangler.toml`

## Out Of Scope Files

- `docs/research/notes.md`
- `wrangler.toml`

## Evidence

- `the drifted git change still updates site files`
- `but blocked research or deploy paths were touched in the same change`

## Reviewer Posture

- inspect the change before approval
- scope drift is visible and must be understood
- evidence is attached to support the change
