# Intent

Last updated: 2026-03-12
Task status: completed

## Change Intent

Create a canonical public build for Specpunk and make it deployable on `specpunk.com` without adding a new build system.

## Must Preserve

- the public surface should remain an honest static object, not a fake SaaS landing page
- the visual direction should stay consistent with the current editorial control surface
- the page should remain lightweight and static
- the prompt surface should stay local and non-submitting

## Must Not Introduce

- a new frontend framework
- a fake lead funnel
- hidden backend behavior
- unrelated changes to research documents

## Success Condition

The public surface is represented by a small static output, can be deployed through Cloudflare Pages, and reads as the current product front door.
