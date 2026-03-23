# Evidence

Last updated: 2026-03-12
Task status: completed

## Repo Evidence

- the first scene now includes `open artifact drawer`
- the artifact pack includes a toolbar with `open required`, `expand all`, and `collapse all`
- required artifacts are tagged in markup and grouped through the drawer logic
- the artifact section can pulse and focus as a single review object

## Behavioral Evidence

- `open artifact drawer` scrolls to the artifact section and opens the required artifact set
- `open required` opens only the minimal boundary artifacts
- `expand all` opens every artifact
- `collapse all` closes all artifacts
- the conversation link to `open artifact pack` now uses the same drawer flow instead of a passive anchor

## Validation Notes

- browser sanity checks should confirm the three drawer states
- the change does not require a new build step or deployment shape
- the drawer remains local UI state only

## Deployment Evidence

- the updated public surface was redeployed through Cloudflare Pages
- current production deployment after this change:
  `https://879b984e.specpunk-8ka.pages.dev`
