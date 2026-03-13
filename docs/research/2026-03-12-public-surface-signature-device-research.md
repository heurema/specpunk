---
title: "Specpunk Public Surface Signature Device Research"
date: 2026-03-12
status: partially_verified
origin: delve pass on the missing memorable element in the public-surface prototype
---

# Question

What distinctive element should Specpunk add if the first screen still feels under-anchored, too empty at the bottom, and missing one memorable purposeful device?

## Executive Summary

Specpunk should **not** use a true mascot as its primary identity.

The stronger move is a `functional signature device`:

- one mark
- one meaning
- multiple placements

That device should carry the product's own semantics of:

- `boundary`
- `containment`
- `evidence`
- `review posture`

and it should also solve the spatial problem the user called out:

- the lower part of the first scene feels visually unclaimed

The best v1 recommendation is:

1. `Boundary Bracket` as the core mark
2. `Containment Seal` only as a secondary scene object if the page still feels too dry
3. `Margin Sentinel` as a supporting editorial device, not the primary identity

## What Sources Support Directly

### 1. Serious technical brands sharply limit mascot usage

GitHub's first-party mascot guidance is unusually explicit:

- `less is more`
- overuse or use as space fillers can be `distracting and annoying`
- mascots should be used `sparingly`
- only when `the context is appropriate`
- not as logos
- not to `explain, interrupt, or sell`
- not for `money, security, sales, enterprise offerings, apologies, politics or crises`

This is highly relevant to Specpunk because the product sits close to:

- review
- control
- reliability
- trust

Those are exactly the contexts where GitHub warns against mascot-led expression.

### 2. Strong recognition in technical products usually comes from symbol + rules

GitHub's brand toolkit says:

- `Our logo represents our brand and helps people recognize us at a glance.`
- it is a `key part of our identity`
- it should be used `consistently`
- it should sit high in the `visual hierarchy`

Docker's brand guidelines say:

- `The primary logo is made up of two elements: the symbol and the wordmark.`
- `The symbol is available as a secondary logo or icon.`

Docker also fixes:

- minimum size
- clear space
- explicit don'ts such as `Don't use the wordmark alone`

That is the important pattern: memorability comes from disciplined repetition and role clarity, not from attaching a character to every surface.

### 3. Character-led brands still bind characters into a rigid system

Mailchimp's brand assets page says:

- it always pairs the company name with the `Freddie` symbol
- `Cavendish Yellow` is the brand color
- there should be generous space around the mark
- the graphics must not be modified
- the graphics must not imply affiliation or endorsement

So even a more expressive brand still depends on:

- lockup
- color
- clear-space rules
- strict non-modification rules

Duolingo's official brand-guidelines bundle says:

- `Illustration is a key part of our visual identity`
- `Duo is our mascot and most recognizable brand asset`
- `Duo next to our logotype is useful in third party applications, where Duo's recognizability would aid in brand awareness`

That is not random mascot use. It is character use inside a full visual system.

### 4. The public surface should still behave like a service, not a site

This remains aligned with earlier design research.

GOV.UK's principles still matter here:

- `Build digital services, not websites`
- `Do less`
- `Do the hard work to make it simple`

So the missing element should not be decorative filler.

It should behave like part of the tool.

## Codex Synthesis

The missing element has to do three jobs at once.

### Job 1: Memory

It should give the page a strong recall handle.

Not:

- a mascot people merely notice
- a random accent color
- a decorative illustration in dead space

But:

- one repeated thing the eye learns quickly

### Job 2: Meaning

It should encode product semantics, not generic brand mood.

For Specpunk, the strongest meanings are:

- what is inside the allowed boundary
- what has been contained enough to review
- what evidence is attached

### Job 3: Spatial Anchor

It should claim the currently empty lower part of the first scene.

That means it should not live only in the logo.

It should also exist as a deliberate low-screen anchor.

If it cannot do all three jobs, it will probably feel fake.

## Candidate Devices

### 1. `Boundary Bracket`

**Shape**

- a hard `L`
- or paired bracket form like `][`
- or an open containment frame

**Why it fits**

This is the cleanest translation of Specpunk's main idea into form:

- declare a boundary
- make it visible
- hold the change inside it

It already sounds like the product.

It does not need a story.

**Where it should live**

- small version in the `logo/topbar`
- oversized low-opacity version anchored into the lower edge of the `boundary-scene`
- smaller repeats around `review artifact`, `artifact-card`, or `scene-metrics`

**Why it solves the empty-bottom problem**

Because the bracket can partly sit behind or through the lower strip of the first scene, claiming space without needing a block of explanatory copy.

**Best color behavior**

- default: graphite or `contained` teal
- rust only when explicitly representing risk or breach

**Risk**

If drawn too lightly, it disappears.

If drawn too heavily, it starts to feel like dated enterprise software.

### 2. `Containment Seal`

**Shape**

- circular or slightly oval review seal
- coded, not celebratory
- something like `contained / 03 files / evidence attached`

**Why it fits**

It turns the review state into an object.

That makes it memorable for the right reason:

- it is a proof object
- not a brand mascot

**Where it should live**

- crossing the lower edge of the first scene
- smaller versions in artifact or review sections

**Why it solves the empty-bottom problem**

Because it can literally occupy the dead lower zone and make it feel intentional.

**Risk**

It can easily become a fake approval badge.

So it must communicate:

- inspection state
- not success theater

### 3. `Margin Sentinel`

**Shape**

- vertical tab
- docket spine
- index marker with a notch or cut

**Why it fits**

This is the most editorial option.

It helps the whole page feel like one controlled object rather than stacked web sections.

**Where it should live**

- attached to one scene edge
- repeated in notebook and artifact layers as an index device

**Why it solves the empty-bottom problem**

Because it can stretch through the composition and make the page feel structurally held together.

**Risk**

It may be too quiet to become memorable on its own.

## Recommendation

Choose `Boundary Bracket` as the primary signature device.

Reason:

1. It is the closest direct translation of product semantics into form.
2. It can work as `logo`, `icon`, `state marker`, and `bottom anchor`.
3. It stays serious without becoming generic.
4. It matches the current `editorial control surface` direction better than a character or soft illustration.

Use `Containment Seal` only if, after the bracket is in place, the first scene still feels emotionally too dry.

That makes the seal a secondary event object, not the core brand.

## Concrete UI Implications

The chosen device should appear in three scales.

### Micro

In the topbar/lockup.

This is the smallest persistent memory cue.

### Meso

In the lower zone of the first scene.

This is the spatial anchor that fixes the current “too much empty space below” feeling.

### System

Inside `artifact`, `review`, and `scope` elements.

This is what turns the device into a product grammar instead of a logo sticker.

## Explicit Do / Don't

### Do

- make the device functional
- repeat it consistently
- give it strict usage rules
- let it signal containment, inspection, or boundary

### Don't

- turn it into a cartoon face
- use it as filler
- add a new random accent color just to make it memorable
- let it imply blanket approval when evidence is missing

## Suggested Next Step

Do not jump to a final logo system yet.

Instead, test one controlled iteration in the prototype:

1. add `Boundary Bracket` to the topbar lockup
2. add one oversized low-opacity bracket as the bottom anchor of the first scene
3. repeat a smaller bracket in artifact/review headers

Then review whether the page now:

- feels more memorable
- feels less empty at the bottom
- still feels like a product object rather than a site

## Sources

- GitHub Brand Toolkit, `Logo`: `https://brand.github.com/foundations/logo`
- GitHub Brand Toolkit, `Mascots`: `https://brand.github.com/graphic-elements/mascots`
- GitHub Octodex: `https://octodex.github.com/`
- Docker Media Resources / Brand Guidelines: `https://www.docker.com/company/newsroom/media-resources/`
- Mailchimp Brand Assets: `https://mailchimp.com/about/brand-assets/`
- Duolingo Brand Guidelines: `https://design.duolingo.com/`
- GOV.UK Design Principles: `https://www.gov.uk/design-principles`
