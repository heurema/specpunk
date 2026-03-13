# Intent

Last updated: 2026-03-12
Task status: completed

## Change Intent

Create the smallest useful CLI that turns declared scope and actual changed files into a markdown review artifact.

## Must Preserve

- no external dependencies
- no hidden network behavior
- structured input over transcript scraping
- output must stay compact and review-oriented

## Must Not Introduce

- YAML parsing dependency
- framework setup
- generalized automation before the basic boundary logic works
- long or decorative output

## Success Condition

Given a JSON task input, the tool emits a markdown review artifact that makes scope drift or scope compliance obvious.
