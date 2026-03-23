---
title: "Specpunk Public Surface Design Tokens"
date: 2026-03-12
status: working
origin: implementation memo derived from the public surface design research
---

# Specpunk Public Surface Design Tokens

## Purpose

This file turns the design research into implementation-ready tokens.

It is not a full design system.

It is the minimum working token set for:

1. the first-screen `Boundary Demonstration`
2. the `Public Lab Notebook`
3. the `Control Room`

## Design Stance

The surface should read as:

- `editorial control surface`
- `paper before glass`
- `artifact before pitch`

The tokens should bias toward:

- clarity
- restraint
- density
- calm severity

The tokens should avoid:

- glossy SaaS surfaces
- neon AI gradients
- terminal cosplay
- decorative motion

## Typeface Tokens

### Working stack

- `--font-sans`: `Recursive`, `IBM Plex Sans`, `system-ui`, `sans-serif`
- `--font-mono`: `Recursive`, `IBM Plex Mono`, `ui-monospace`, `monospace`
- `--font-editorial`: `Newsreader`, `Iowan Old Style`, `Georgia`, `serif`

### Role mapping

- first-screen premise:
  `Recursive Sans Linear`
- notebook headings:
  `Newsreader`
- notebook/body:
  `Recursive Sans Linear`
- labels, counters, artifact state:
  `Recursive Mono`

### Size scale

- `--text-meta`: `0.75rem`
- `--text-mono`: `0.8125rem`
- `--text-body`: `1.0625rem`
- `--text-body-lg`: `1.125rem`
- `--text-title-sm`: `1.5rem`
- `--text-title-md`: `2rem`
- `--text-premise`: `clamp(2.5rem, 5vw, 4rem)`

### Line-height tokens

- `--leading-tight`: `1.1`
- `--leading-copy`: `1.45`
- `--leading-mono`: `1.35`

### Measure tokens

- `--measure-copy`: `68ch`
- `--measure-rail`: `38ch`

## Color Tokens

### Core palette

- `--color-page`: `#F5F1E8`
- `--color-panel`: `#EAE4D8`
- `--color-panel-strong`: `#E1D7C5`
- `--color-ink`: `#1F1F1B`
- `--color-muted`: `#5A564E`
- `--color-line`: `#CFC7B8`
- `--color-risk`: `#8F3B24`
- `--color-contained`: `#1F5C56`
- `--color-highlight`: `#D9CFBB`

### Functional mapping

- page background:
  `--color-page`
- panel surfaces:
  `--color-panel`
- stronger inset or rail surfaces:
  `--color-panel-strong`
- primary copy:
  `--color-ink`
- secondary copy:
  `--color-muted`
- dividers:
  `--color-line`
- out-of-scope / unstable / warning:
  `--color-risk`
- contained / verified / attached evidence:
  `--color-contained`

### Verified contrast pairs

- `--color-ink` on `--color-page` = `14.67`
- `--color-muted` on `--color-page` = `6.48`
- `--color-risk` on `--color-page` = `6.61`
- `--color-contained` on `--color-page` = `6.84`

## Surface Tokens

- `--border-default`: `1px solid var(--color-line)`
- `--border-strong`: `1.5px solid var(--color-ink)`
- `--radius-card`: `10px`
- `--radius-chip`: `999px`
- `--shadow-none`: `none`
- `--shadow-paper`: `0 1px 0 rgba(31, 31, 27, 0.06)`

### Surface rules

- use borders before shadows
- use paper layering before dark panels
- keep radius low
- avoid blurred glass effects

## Spacing Tokens

- `--space-2`: `0.125rem`
- `--space-4`: `0.25rem`
- `--space-8`: `0.5rem`
- `--space-12`: `0.75rem`
- `--space-16`: `1rem`
- `--space-20`: `1.25rem`
- `--space-24`: `1.5rem`
- `--space-32`: `2rem`
- `--space-40`: `2.5rem`
- `--space-48`: `3rem`
- `--space-64`: `4rem`
- `--space-80`: `5rem`

### Layout tokens

- `--page-max`: `86rem`
- `--boundary-columns`: `1.15fr 0.78fr 1fr`

## Motion Tokens

- `--ease-standard`: `cubic-bezier(0.2, 0.8, 0.2, 1)`
- `--dur-fast`: `120ms`
- `--dur-base`: `180ms`
- `--dur-slow`: `260ms`

### Allowed motion

- compare-state switches
- inline expand/collapse
- drawer open/close
- small state fades

### Forbidden motion

- ambient floating
- decorative parallax
- animated gradients
- terminal cursor theatrics

### Reduced motion

Under `prefers-reduced-motion`, motion should become:

- opacity-only
- or instant state change

## Component Tokens

### Boundary panel

- padded paper card
- low radius
- strong header label
- metric row in mono
- list density high enough to feel operational

### Notebook card

- more white space than artifact panels
- editorial heading
- short body copy
- mono metadata

### Artifact card

- mono summary row
- inline code or short text block
- details/summary interaction acceptable

### Chip

- mono label
- low contrast fill by default
- strong semantic accent only for `risk` and `contained`

## CSS Variable Block

```css
:root {
  --color-page: #F5F1E8;
  --color-panel: #EAE4D8;
  --color-panel-strong: #E1D7C5;
  --color-ink: #1F1F1B;
  --color-muted: #5A564E;
  --color-line: #CFC7B8;
  --color-risk: #8F3B24;
  --color-contained: #1F5C56;
  --color-highlight: #D9CFBB;

  --font-sans: "Recursive", "IBM Plex Sans", system-ui, sans-serif;
  --font-mono: "Recursive", "IBM Plex Mono", ui-monospace, monospace;
  --font-editorial: "Newsreader", "Iowan Old Style", Georgia, serif;

  --text-meta: 0.75rem;
  --text-mono: 0.8125rem;
  --text-body: 1.0625rem;
  --text-body-lg: 1.125rem;
  --text-title-sm: 1.5rem;
  --text-title-md: 2rem;
  --text-premise: clamp(2.5rem, 5vw, 4rem);

  --leading-tight: 1.1;
  --leading-copy: 1.45;
  --leading-mono: 1.35;

  --measure-copy: 68ch;
  --measure-rail: 38ch;

  --border-default: 1px solid var(--color-line);
  --border-strong: 1.5px solid var(--color-ink);
  --radius-card: 10px;
  --radius-chip: 999px;
  --shadow-paper: 0 1px 0 rgba(31, 31, 27, 0.06);

  --space-4: 0.25rem;
  --space-8: 0.5rem;
  --space-12: 0.75rem;
  --space-16: 1rem;
  --space-20: 1.25rem;
  --space-24: 1.5rem;
  --space-32: 2rem;
  --space-40: 2.5rem;
  --space-48: 3rem;
  --space-64: 4rem;

  --page-max: 86rem;
  --boundary-columns: 1.15fr 0.78fr 1fr;

  --ease-standard: cubic-bezier(0.2, 0.8, 0.2, 1);
  --dur-fast: 120ms;
  --dur-base: 180ms;
  --dur-slow: 260ms;
}
```

## Recommendation

For the first implementation, keep the tokens exactly this strict.

Do not add:

- a second accent color
- glossy shadows
- larger radii
- dark mode
- hero gradients

If the first prototype already feels coherent with this constraint set, the direction is strong.
