# First Dogfood Task

Last updated: 2026-03-12
Owner: Vitaly
Status: completed

## Task

Turn the current public surface into a deploy-ready static site and publish it through Cloudflare Pages for `specpunk.com`.

## Why This Task

This is the first real completed task in the repo that matches the product thesis:

- it has a clear boundary
- it can be described with compact artifacts
- it produces a reviewable outcome
- it is not hypothetical

## Runtime Scope

The runtime task covered only the publishable public surface and deploy configuration.

Allowed repo scope:

- `site/index.html`
- `site/style.css`
- `site/_headers`
- `wrangler.toml`

Meta-doc scope for the dogfood packet itself is separate and does not count as runtime product scope.

## Outcome

- a canonical static site now exists under `site/`
- the site is deployable through Cloudflare Pages
- the project `specpunk` exists in the Cloudflare account that owns `specpunk.com`
- the custom domain was accepted and marked active by the Pages API

## Artifact Links

- [intent.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-first-dogfood-task/intent.md)
- [scope.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-first-dogfood-task/scope.md)
- [evidence.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-first-dogfood-task/evidence.md)
- [review.md](/Users/vi/personal/specpunk/docs/product/tasks/2026-03-12-first-dogfood-task/review.md)

## What This Proves

Specpunk can already describe one real task in this repo as a bounded review object instead of a raw diff plus chat history.

That is a small proof, not full product validation.
But it is the first working loop of:

`intent -> scope -> evidence -> review`
