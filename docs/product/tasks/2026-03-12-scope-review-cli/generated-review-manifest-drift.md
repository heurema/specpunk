# Generated Review Artifact

Task: Add a minimal artifact drawer to the public surface without touching deploy configuration or research docs.
Decision: inspect
Reason: blocked files were touched

## Scope Summary

- Declared allowed patterns: 2
- Declared blocked patterns: 3
- Changed files: 4
- In scope: 2
- Out of scope: 2
- Blocked touched: 2
- Scope status: drifted

## Allowed Patterns

- `site/index.html`
- `site/style.css`

## Blocked Patterns

- `site/_headers`
- `wrangler.toml`
- `docs/research/**`

## Changed Files

- `site/index.html`
- `site/style.css`
- `docs/research/2026-03-12-public-surface-brief.md`
- `wrangler.toml`

## Out Of Scope Files

- `docs/research/2026-03-12-public-surface-brief.md`
- `wrangler.toml`

## Evidence

- `drawer behavior works on the public surface`
- `but unrelated config and research files were touched`

## Reviewer Posture

- inspect the change before approval
- scope drift is visible and must be understood
- evidence is attached to support the change
