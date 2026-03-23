---
title: "Specpunk Public Surface Design Research"
date: 2026-03-12
status: partially_verified
origin: delve pass on visual language, typography, composition, color, and motion for the Specpunk public surface
---

# Specpunk Public Surface Design Research

## Question

What design direction should the Specpunk public surface take if it must stay simple, coherent, honest, memorable, and aligned with the user's rejection of conventional startup landing-page grammar?

## Executive Summary

The strongest direction is not a trendy `AI landing page`.

It is an `editorial control surface`:

- light, warm, paper-like base
- dark graphite text
- one strict technical type system
- one restrained human accent
- strong grid and borders instead of glossy cards
- motion only where the user learns a state change

The clearest recommended stack is:

1. `Boundary Demonstration` as the first screen
2. `Public Lab Notebook` as the second layer
3. `Control Room` as the deep layer

Visually, the best primary type direction is:

- `Recursive` as the core system
- `Newsreader` only as a sparse notebook accent

If you want maximum implementation simplicity, the best fallback is:

- `Recursive` only

If you want maximum sobriety at the cost of some memorability, the best alternative is:

- `IBM Plex` family

The page should feel closer to:

- a review console
- a research notebook
- a live artifact

and farther from:

- a polished SaaS homepage
- a neon dashboard
- a terminal cosplay site

## What Sources Support Directly

### 1. Simplicity here should mean usefulness and honesty, not emptiness

`Vitsœ`'s presentation of Dieter Rams is directly relevant:

- good design makes a product useful
- good design is understandable
- good design is unobtrusive
- good design is honest
- good design is as little design as possible

This matters because Specpunk should not look under-designed or over-designed. It should look like a tool whose shape has been reduced to its irreducible core.

### 2. The surface should behave more like a service than a website

`GOV.UK`'s principles map unusually well onto this task:

- start with user needs
- do less
- do the hard work to make it simple
- build digital services, not websites
- be consistent, not uniform
- make things open

This supports the earlier product conclusion: the public surface should behave like an object people inspect, not a funnel they are pushed through.

### 3. Typography will carry a large share of the perceived quality

`Practical Typography` gives concrete, web-usable rules:

- body text on the web is usually comfortable around `15-25 px`
- line spacing is best around `120-145%`
- line length should usually land around `45-90` characters
- typography quality is determined largely by the body text

That is especially important here because the page will include notebook text, artifact labels, short explanations, and system-state cards. If the body text is weak, the whole object will feel weak.

### 4. Motion should be functional and user-governed

`MDN` says `prefers-reduced-motion` exists to detect when the user wants to minimize non-essential motion, so interfaces can remove, reduce, or replace motion-based animation.

So if motion exists, it has to teach a state change:

- raw versus controlled change
- collapsed versus expanded explanation
- hidden versus opened artifact

Not ambient polish.

### 5. Contrast needs to stay explicit and measurable

`WCAG 2.1` sets the hard baseline:

- `4.5:1` for normal text
- `3:1` for large-scale text
- no friendly rounding

That means a light, paper-like surface is viable, but only if the palette stays disciplined.

## Typography Research

### Primary Recommendation: `Recursive` core + `Newsreader` accent

This is the strongest overall fit.

Why `Recursive` is unusually good here:

- it is explicitly built for `design, code, and UI`
- it includes both `Sans` and `Mono`
- its shared metrics across the `Monospace` axis support harmonious layouts in `data-rich applications and technical documentation`
- its internal contrast is enough to create multiple roles while keeping a coherent voice
- its `Linear` end is optimized for readability and dense information

This makes it a rare type system that can support all of these without changing personality too aggressively:

- control rail
- artifact panels
- notebook body text
- labels and metadata
- code-ish fragments

Why add `Newsreader` at all:

- Google Fonts describes it as intended for `continuous on-screen reading in content-rich environments`
- it can give the notebook layer a human, editorial pressure without contaminating the rest of the interface

Important limit:

`Newsreader` should stay sparse.

Use it for:

- notebook section titles
- occasional thesis headings
- maybe one pull-quote or one strong line

Do not use it for:

- the control rail
- dense body copy everywhere
- artifact labels
- first-screen system state

### Fallback: `Recursive` only

This is the best version if you want raw v1 speed and maximum coherence.

It will be:

- simpler to ship
- easier to tune
- harder to make feel warm

But it can still work very well if you use the family deliberately:

- `Recursive Sans Linear` for most copy
- `Recursive Mono` for labels, diffs, counters, and system state
- limited use of `Casual` only for tiny moments of humanity, not for the main UI

### Sober Alternative: `IBM Plex` family

`IBM Plex` is a very defensible alternative if you want more infrastructural seriousness.

Why it works:

- global
- versatile
- includes sans, mono, and serif
- technically credible

Why it is not the first recommendation:

- it is less distinctive
- it risks landing as `credible infrastructure` more than `new object worth inspecting`

### Why not `Fraunces` as the main accent

`Fraunces` is explicitly described as a display soft-serif.

That makes it useful as a reference for tone, but not as a strong primary choice here.

It will likely push the page too quickly toward:

- stylization
- mood
- display-led personality

before the product has established its utility.

## Concrete Typography System

### Recommended v1 roles

- first-screen premise line:
  `Recursive Sans Linear`, `40-48 px`, `1.05-1.15 line-height`, medium or semibold
- notebook section titles:
  `Newsreader`, `24-32 px`, tight line-height
- notebook/body copy:
  `Recursive Sans Linear`, `17-18 px`, `1.45 line-height`, max width `62-72ch`
- artifact labels and system state:
  `Recursive Mono`, `13-14 px`, `1.35 line-height`
- small metadata:
  `Recursive Mono`, `12-13 px`

### Typographic rules

- keep prose measure within `62-72ch`
- keep dense rails tighter, around `32-42ch`
- do not use giant headlines to manufacture drama
- let contrast come from role, weight, and family, not from scale alone
- mono text should signal precision, not nostalgia

## Composition Research

### Recommended composition

The strongest structure remains:

1. `Boundary Demonstration`
2. `Public Lab Notebook`
3. `Control Room`

But the design research sharpens how this should feel.

### Layer 1: Boundary Demonstration

This should be almost a full-screen object.

Not a hero section with benefits.

A stricter arrangement:

- left: uncontrolled state
- middle: control rail
- right: contained state

The emotional logic:

- left side creates tension
- middle side explains why
- right side shows containment

This is where the first line belongs:

`This change should touch 3 files. It touched 11.`

Short. Factual. Severe.

### Layer 2: Public Lab Notebook

This should not look like a blog feed.

It should feel like a living technical notebook with short entries:

- thesis delta
- latest benchmark result
- open question
- what changed this week

Each entry should be compact and decision-rich.

The notebook layer is where the page becomes human without becoming promotional.

### Layer 3: Control Room

This is the deepest layer and the most literal product surface.

Use drawers, tabs, or indexed panels for:

- `intent.md`
- `glossary.md`
- `invariants.md`
- `scope.yml`
- `evidence.md`
- `review.md`

This should feel like opening a real artifact, not reading a feature card.

## Visual Language

### Recommended direction

Call the direction:

`Editorial Control Surface`

That means:

- editorial restraint
- system precision
- paper before glass
- borders before shadows
- contrast before spectacle

### Material system

Use:

- warm off-white page background
- slightly darker paper panels
- hairline dividers
- almost no shadow
- very small or zero radius

The surface should read as:

- layered paper
- pinned notes
- reviewed artifact

not:

- glass dashboard
- futuristic operating system
- startup gradient theater

### Recommended palette

Suggested base palette:

- page background: `#F5F1E8`
- panel background: `#EAE4D8`
- primary text: `#1F1F1B`
- secondary text: `#5A564E`
- risk accent: `#8F3B24`
- contained accent: `#1F5C56`
- divider line: `#CFC7B8`

This creates:

- warmth without nostalgia
- severity without black-box darkness
- memorability without neon gimmicks

### Local contrast check

The following pairings clear the `4.5:1` WCAG threshold in a local luminance calculation:

- `#1F1F1B` on `#F5F1E8` = `14.67`
- `#5A564E` on `#F5F1E8` = `6.48`
- `#8F3B24` on `#F5F1E8` = `6.61`
- `#1F5C56` on `#F5F1E8` = `6.84`

So the palette can stay quiet without becoming low-contrast mush.

## Motion System

### Principle

Motion should explain state, not decorate brand.

Use motion only when one of these becomes clearer because it moved:

- a diff becomes scoped
- a warning is expanded
- an artifact drawer opens
- a comparison scrub reveals the contained state

### Recommended motion types

- inline expand/collapse
- horizontal compare scrub
- drawer open/close
- minimal notebook card reveal

### What to avoid

- floating cards
- ambient background motion
- animated gradients
- theatrical parallax
- terminal cursor gimmicks

### Reduced-motion behavior

Under `prefers-reduced-motion`:

- replace transform-heavy motion with opacity or immediate swaps
- keep the same information order
- do not hide meaning inside animation

## What Will Make It Memorable

Not novelty for novelty's sake.

The memorable part should come from:

- one strong factual first line
- an object-like first screen
- coherent type voice
- disciplined palette
- unusually honest notebook layer

In other words:

people should remember the page because it felt like a real instrument, not because it tried to perform originality.

## Recommended Design Direction

If only one direction should move into implementation, use this:

### Direction A: `Editorial Control Surface`

- first screen:
  three-rail comparison with strong factual premise
- typography:
  `Recursive` core, `Newsreader` accent
- palette:
  warm paper + graphite + rust + deep teal
- surface:
  borders, paper panels, no glossy cards
- navigation:
  `notebook`, `artifact pack`, `conversation`
- motion:
  only stateful and skippable

This is the best balance of:

- simplicity
- coherence
- memorability
- honesty
- implementation realism

## Directions To Reject For v1

### 1. Dark cyberpunk dashboard

Wrong because it implies:

- spectacle
- security-theater
- terminal roleplay

instead of review clarity and control.

### 2. Soft gradient AI landing page

Wrong because it implies:

- generic optimism
- product vagueness
- startup sameness

instead of an inspectable object.

### 3. Display-serif-first editorial site

Wrong because it over-indexes on:

- taste
- curation
- visual personality

before the product has proven itself as a tool.

## Verification Status

### Verified

- typography baselines from `Practical Typography`
- contrast thresholds from `WCAG 2.1`
- reduced-motion semantics from `MDN`
- the design principles used from `Vitsœ` and `GOV.UK`
- factual properties of `Recursive`, `IBM Plex`, `Newsreader`, and `Fraunces`
- local contrast ratios for the proposed palette

### Synthesis

- the exact palette
- the exact composition
- the final font recommendation
- the `Editorial Control Surface` design direction
- the specific role mapping of type, motion, and interaction

These are not direct source facts. They are the recommended design synthesis for Specpunk based on the verified constraints plus the user's stated preferences.

## Source List

- Practical Typography: https://practicaltypography.com/typography-in-ten-minutes.html
- Practical Typography, line length: https://practicaltypography.com/line-length.html
- Practical Typography, line spacing: https://practicaltypography.com/line-spacing.html
- Vitsœ / Dieter Rams principles: https://www.vitsoe.com/us/about/good-design
- GOV.UK Design Principles: https://www.gov.uk/guidance/government-design-principles
- WCAG 2.1 contrast guidance: https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum.html
- MDN prefers-reduced-motion: https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-reduced-motion
- Recursive Sans & Mono: https://recursive.design/
- IBM Plex: https://www.ibm.com/plex/
- Google Fonts / Newsreader: https://fonts.google.com/specimen/Newsreader
- Google Fonts / Fraunces: https://fonts.google.com/specimen/Fraunces
- Google Fonts / IBM Plex Mono: https://fonts.google.com/specimen/IBM+Plex+Mono
- htmx homepage, observational reference: https://htmx.org/

## Recommended Next Step

Do not jump straight into final polished UI.

Do this next:

1. build one low-fidelity HTML prototype of the `Boundary Demonstration`
2. implement the recommended palette and type system first
3. add only one notebook strip and one artifact drawer
4. test whether the page still reads correctly with almost no explanatory copy

If that prototype already feels like a real object, the direction is right.
