# Scope

Last updated: 2026-03-12
Task status: completed

## Declared Runtime Scope

Allowed:

- `site/index.html`
- `site/style.css`
- `site/_headers`
- `wrangler.toml`

Blocked:

- `docs/research/**`
- `docs/product/**`
- `docs/prototypes/**`
- any new app framework or build tool

## Actual Runtime Scope

Touched:

- `site/index.html`
- `site/style.css`
- `site/_headers`
- `wrangler.toml`

External system touched:

- Cloudflare Pages project `specpunk`
- custom domain `specpunk.com`

## Scope Result

Status: respected

The runtime change stayed inside the declared repo boundary.

The dogfood documentation created after the task is meta-work and is not counted as runtime scope drift.
