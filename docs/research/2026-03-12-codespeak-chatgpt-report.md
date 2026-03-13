---
title: "CodeSpeak Research Summary from ChatGPT"
date: 2026-03-12
origin: user-provided ChatGPT summary pasted into chat
status: imported-summary
note: This file preserves the summary text pasted by the user. The original sandbox file paths referenced in chat are not available in this workspace.
---

# CodeSpeak Research Summary from ChatGPT

Original note from chat:

> Да, провёл. Полный отчёт: `codespeak_deep_research_report.md`. Исходный пакет промптов для агентов: `codespeak_research_prompt_pack.md`.
>
> Ниже — сжатая выжимка.

## Факты

Рынок уже ушёл от простого autocomplete в сторону agentic coding и spec-centered workflows. В Stack Overflow 2025 среди AI agent/tools для разработки чаще всего называли ChatGPT, GitHub Copilot и Claude Code; при этом лишь 17.5% используют AI-агентов на работе ежедневно, а 52% сообщают о положительном влиянии на productivity. Это похоже на фазу активного внедрения, но не на полностью зрелый settled market. ([Stack Overflow][1])

Главная подтверждённая боль — trust/review gap. Stack Overflow фиксирует 46% недоверия к точности AI, Sonar — 96% неполного доверия к AI-generated code, 61% считают, что такой код часто выглядит правильным, но ненадёжен; JetBrains отдельно перечисляет inconsistent quality, слабое понимание complex logic, lack of context awareness и privacy/security как главные опасения. ([Stack Overflow][1])

Бутылочное горлышко смещается из generation в verification. Anthropic пишет, что при росте code output примерно на 200% review стал узким местом, и поэтому они запустили multi-agent code review; DORA предупреждает, что AI усиливает и сильные, и слабые стороны организации и может увеличивать нестабильность через крупные, тяжело ревьюимые батчи. ([Claude][2])

По публичному следу CodeSpeak делает ставку не на «ещё одного агента», а на intent/spec layer. На сайте продукт описан как “AI language built for humans”, обещает держать production-grade systems в спеках “5–10x smaller”, поддерживает mixed projects и замену части существующего кода спеками. В takeover-flow есть `init --mixed` и `takeover`, а в свежих обновлениях — imports/dependencies, managed files и scope control. Это очень похоже на стратегию `durable intent + brownfield takeover + controlled change`. ([CodeSpeak][3])

У CodeSpeak verification уже вынесен в отдельную ось: `codespeak coverage` запускает тесты, измеряет coverage и добавляет тесты; в их блоге прямо сказано, что AI code generation настолько хорош, насколько хорош test suite, который проверяет изменения. Значит, они целятся не только в генерацию, но и в evidence layer поверх неё. ([CodeSpeak][4])

Похожие подходы уже есть, но фокус у них разный. Kiro формализует feature workflow через `requirements.md`, `design.md`, `tasks.md` и усиливает это property-based correctness checks; Augment Intent держит spec в центре и обновляет её по факту работы агентов; GitHub Spec Kit продвигает executable specs и development constitution; Tessl тоже двигается от spec-first в сторону spec-as-source. Категория уже существует, но winning shape ещё не устоялся. ([Kiro][5])

Критический риск направления тоже уже виден: Martin Fowler и Birgitta Böckeler прямо отмечают, что SDD может скатиться в overkill, где generated markdown становится не лучше generated code. Это сильный аргумент в пользу минимальных, decision-rich spec-артефактов вместо длинных автоматически сгенерированных документов. ([martinfowler.com][6])

Brownfield — самый важный и самый трудный слой. CodeSpeak уже показывает code→spec takeover, а исследования по reverse engineering user stories from code подтверждают, что LLM способны хотя бы частично восстанавливать intent из кода на ограниченных сценариях. Но METR показывает и пределы текущих агентных систем: в их исследовании опытные OSS-разработчики с frontier AI работали на 19% дольше, а test-passing PR ещё не гарантирует, что maintainers захотят его принять. ([CodeSpeak][7])

Исторически это продолжение старой линии: literate programming у Knuth, language workbenches вроде JetBrains MPS и попытки Intentional Software поднять уровень абстракции над кодом. AI не создал проблему заново; он сделал её коммерчески срочной. ([Computer Science][8])

## Мой вывод

Самый сильный незакрытый white space сейчас не в тезисе «полностью программировать на естественном языке», а в связке из четырёх вещей: `minimal durable spec`, `brownfield takeover`, `semantic consistency / contradiction checks` и `behavior-first review`. Эта связка частично покрыта разными игроками, но целиком пока не выглядит закрытой. ([CodeSpeak][7])

Ассумпция: публичные материалы CodeSpeak могут не отражать весь внутренний roadmap, поэтому вывод выше — это реверс-инжиниринг по внешним сигналам, а не знание их внутренней стратегии.

## Наиболее сильная product thesis для своего решения

Не строить новый IDE с нуля, а сделать слой поверх существующих agentic tools: `module intent card + glossary/invariants + allowed edit scope + cross-boundary warnings + behavior/test evidence`. Это лучше совпадает с тем, как уже работают Claude Code, Codex и Cursor: у них уже есть task execution, planning, workspace guidance, review practices и long-running multi-file flows, поэтому поверх них проще внедрять durable intent и safer change control, чем просить команду полностью сменить toolchain. ([Claude][9])

## Что я бы дал следующим агентам

1. Competitive teardown по 8–10 игрокам: CodeSpeak, Kiro, Augment Intent, Spec Kit, Tessl, Cursor, Claude Code, Codex.
2. Brownfield benchmark на 5–10 OSS-репо: code → intent pack → change → regen → review cost.
3. Прототип terminology/contradiction checker для natural-language specs.
4. Эксперимент review UX: raw diff vs spec diff + test/behavior evidence.
5. Customer discovery у команд, которые уже живут на Claude Code / Cursor / Copilot и упираются в review/context drift.

Самое важное: шанс здесь не в том, чтобы писать **больше** кода агентами, а в том, чтобы сделать **intent короче, устойчивее и проверяемее, чем код**.

[1]: https://survey.stackoverflow.co/2025/ai
[2]: https://claude.com/blog/code-review
[3]: https://codespeak.dev/
[4]: https://codespeak.dev/blog/coverage-20260302
[5]: https://kiro.dev/docs/specs/
[6]: https://martinfowler.com/articles/exploring-gen-ai/sdd-3-tools.html
[7]: https://codespeak.dev/blog/codespeak-takeover-20260223
[8]: https://cs.stanford.edu/~knuth/lp.html
[9]: https://code.claude.com/docs/en/overview
