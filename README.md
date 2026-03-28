```
                       __
    ____  __  ______  / /__
   / __ \/ / / / __ \/ //_/
  / /_/ / /_/ / / / / ,<
 / .___/\__,_/_/ /_/_/|_|
/_/
```

**Agent orchestration platform for solo founders running AI agent fleets.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-334%20passing-brightgreen.svg)]()

> Think Paperclip, but CLI-first. No dashboard, no database, no API keys required.

---

## Install

```sh
cargo install --git https://github.com/heurema/specpunk punk-run
```

Or build from source:

```sh
git clone https://github.com/heurema/specpunk
cd specpunk/punk
cargo install --path punk-run
```

## Quick Start

```sh
# 1. Check providers are available
punk-run doctor

# 2. Configure projects and agents
mkdir -p ~/.config/punk
# Edit ~/.config/punk/projects.toml, agents.toml, policy.toml

# 3. Queue a task
punk-run queue myproject "add input validation to the API" --agent claude

# 4. Start the daemon (dispatches tasks to AI agents)
punk-run daemon

# 5. Check status
punk-run status
punk-run morning
```

## What It Does

Specpunk dispatches tasks to AI coding agents (Claude, Codex, Gemini), tracks their work through structured receipts, enforces budgets, and runs autonomous goal-driven execution.

**25 commands across 6 areas:**

```sh
# Task dispatch
punk-run queue <project> "prompt"         # one-off task
punk-run daemon                           # start dispatcher
punk-run status                           # what's running

# Goals (autonomous cycle)
punk-run goal create <project> "objective" # planner generates steps
punk-run goal approve <id>                # human approves, daemon executes
punk-run goal list                        # track progress

# Operations
punk-run morning                          # daily briefing
punk-run triage                           # review failed tasks
punk-run ask "what is blocking signum?"   # AI query over state
punk-run receipts --since 7               # receipt history

# Multi-model
punk-run diverge <project> "spec"         # 3 providers, compare solutions
punk-run panel "question"                 # ask all, compare answers

# Pipeline CRM
punk-run pipeline list                    # opportunities
punk-run pipeline add <project> <contact> # new lead

# System
punk-run doctor                           # health check
punk-run config                           # show configuration
punk-run policy-check <project>           # test routing rules
punk-run ratchet                          # weekly performance comparison
punk-run graph cost                       # cost chart
```

## How It Works

```
You (CEO)
  |
  punk-run goal signum "prepare checkpoint"
  |
  Planner (Claude Opus) --> generates 5-15 step plan
  |
  You approve plan
  |
  Daemon dispatches steps autonomously:
    Step 1 --> codex-auto (research)
    Step 2 --> claude-coder (implement)
    Step 3 --> claude-reviewer (review)
    ...
  |
  punk-run morning   <-- daily briefing
  punk-run status    <-- what happened
```

**Key design choices:**
- **Flat files, no database** - state lives in JSONL + JSON files, grep-able
- **Subscription billing** - uses CLI tools on flat-rate plans, not per-token API
- **Filesystem bus** - directories as state machine (new/ -> cur/ -> done/)
- **Receipt-driven** - every task produces a structured receipt with cost, duration, status

## Configuration

```
~/.config/punk/
  projects.toml   # which codebases agents work on
  agents.toml     # agent configurations (provider + model + role)
  policy.toml     # routing rules, budgets, feature flags
```

## Architecture

```
specpunk/punk/
  punk-core/    # verification library (14K LOC, frozen)
  punk-cli/     # punk binary (init, plan, check, receipt)
  punk-orch/    # orchestration library (22 modules)
  punk-run/     # punk-run binary (25 commands)
```

## Requirements

- Rust stable
- At least one AI CLI: [Claude Code](https://claude.ai/download), [Codex](https://github.com/openai/codex), or [Gemini CLI](https://github.com/google-gemini/gemini-cli)
- Subscription plan (Claude Max, ChatGPT Pro, or Google AI Pro)

## License

MIT
