# Technical Deep Dive: Key Findings from 4 Reference Repos

## 1. Memory & Context — What Works

### Hermes: Frozen Snapshot Pattern (BEST PRACTICE)
- MEMORY.md + USER.md loaded ONCE at session start into `_system_prompt_snapshot`
- Mid-session writes go to disk immediately but DO NOT update the prompt snapshot
- Preserves Anthropic prefix KV cache = ~75% cost reduction on multi-turn
- Atomic writes: temp file + `os.replace()`, never truncate-in-place
- Security scan before injection (10 regex patterns for prompt injection + invisible Unicode)
- Character limits: MEMORY.md 2200 chars, USER.md 1375 chars (not tokens - model-independent)

**Takeaway for specpunk:** Session context.json should be read once at task start, never mutated mid-task.

### Hermes: Context Compression Algorithm
- Triggers at 50% of context window
- 5-phase: prune old tool results > protect head (3 msgs) > protect tail (by token budget) > LLM summary > orphan sanitization
- Summary: 7-section template (Goal/Constraints/Progress/Decisions/Files/Next Steps/Critical Context)
- Iterative on subsequent compressions: merges new turns into existing summary
- Summary budget: content_tokens * 0.20, floor 2000, cap min(context*0.05, 12000)

**Takeaway for specpunk:** We don't need compression (tasks are short-lived), but the structured summary template is useful for session context between tasks.

### Paperclip: Session Compaction with Handoff Markdown
- Three thresholds: maxSessionRuns (200), maxRawInputTokens (2M), maxSessionAgeHours (72h)
- Claude/Codex classified as "nativeContextManagement: confirmed" - thresholds disabled (they manage their own windows)
- Handoff markdown template: previous session ID, issue ID, rotation reason, last run summary
- Injected into next run's prompt by the adapter

**Takeaway for specpunk:** Claude/Codex manage their own context. Our session context is for cross-task continuity, not in-session compression.

### Paperclip: Context Snapshot (Rich Payload)
Every heartbeat run carries a contextSnapshot with:
- issueId, taskId, taskKey, wakeReason, wakeCommentId
- paperclipWorkspace (cwd, source, mode, strategy, projectId, worktreePath, agentHome)
- paperclipRuntimeServices (live dev server URLs)
- paperclipSessionHandoffMarkdown
- paperclipRuntimeSkills (symlinked skill directories)

**Takeaway for specpunk:** Our task.json already carries most of this. Add session handoff and runtime services.

---

## 2. Inter-Agent Communication

### Paperclip: Tasks + Comments as the Only Channel
- No direct agent-to-agent RPC
- Agent A creates task assigned to Agent B = delegation
- Agent A comments on task mentioning @Agent B = coordination
- Comment handler triggers `heartbeat.wakeup()` for mentioned agents
- @mentions parsed via regex `/@([^\s@,!?.]+)/g` + structured `[[agent:uuid]]` syntax
- Wakeup coalescing: duplicate wakeups merged via `coalescedCount`, context snapshots shallow-merged
- Special case: comment on RUNNING task creates new queued run instead of coalescing (comments don't get swallowed)

**Takeaway for specpunk:** For solo founder, inter-agent comms not needed yet. When needed: use task dependencies + receipt chain, not a message bus.

### Superpowers: Subagent Status Protocol
- 4 statuses: DONE, DONE_WITH_CONCERNS, NEEDS_CONTEXT, BLOCKED
- Controller provides full task text (no file reading by subagent)
- Subagent gets exactly what it needs, nothing inherited from controller session
- Spec reviewer is adversarial by default: "do not trust their claims about completeness"
- Two-stage review: spec compliance THEN code quality (never reversed)

**Takeaway for specpunk:** Adopt the 4-status protocol for task receipts. Add a `concerns` field.

---

## 3. Multi-Model Routing

### Hermes: Smart Model Routing Heuristic
- Route to cheap model ONLY when ALL conditions pass:
  - <= 160 chars, <= 28 words, <= 1 newline
  - No backticks, no URLs
  - No word from 44-term keyword blacklist (debug, implement, refactor, analyze, architecture, etc.)
- Single complex keyword vetoes cheap routing
- On any exception resolving cheap model: silently fall back to primary
- Label in output: "smart route -> gemini-flash-1.5 (openrouter)"

**Takeaway for specpunk:** We route per-task, not per-turn. But the keyword blacklist pattern is useful for auto-categorization.

### Hermes: Delegation Credentials
- Subagents can use different provider:model than parent
- `delegation.base_url` / `delegation.provider` in config
- If not set: inherit everything from parent
- Blocked tools: delegate_task, clarify, memory, send_message, execute_code
- Max 3 concurrent children, max depth 2
- Budget fully isolated (fresh IterationBudget per child)

**Takeaway for specpunk:** Budget isolation is critical (Hermes's shared budget was a bug). Our receipt-per-task model handles this naturally.

---

## 4. Adapter Implementation Details

### Paperclip: claude_local Adapter
- Invokes: `claude --print - --output-format stream-json --verbose`
- Resume: `--resume <sessionId>` only if saved cwd matches current cwd
- Skills: tmpdir with symlinks, passed via `--add-dir skillsDir`
- Prompt: sent via stdin
- 20+ env vars set (PAPERCLIP_AGENT_ID, _COMPANY_ID, _API_URL, _API_KEY, _RUN_ID, _TASK_ID, _WAKE_REASON, etc.)
- Unknown session retry: if exit matches pattern, retry with fresh session
- Billing detection: ANTHROPIC_API_KEY present = "api" billing, else "subscription"

**Takeaway for specpunk:** We already do similar in punk-dispatch.sh. Key additions: session resume by cwd match, env var contract for agent context.

### Hermes: Dangerous Command Approval
- 22 regex patterns: rm -r, chmod 777, DROP TABLE, curl|bash, fork bombs, etc.
- Unicode NFKC normalization before matching (defeats fullwidth Latin obfuscation)
- ANSI strip + null byte strip
- 3 modes: manual (prompt), smart (LLM judge), off
- Smart mode: auxiliary LLM returns APPROVE/DENY/ESCALATE in one word
- 4 user choices: once, session, always (persisted to config.yaml), deny
- Container environments auto-approved (docker, singularity, modal, daytona)

**Takeaway for specpunk:** Our risk tier system (T1/T2/T3) covers this at the task level, not command level. For T1 tasks, we trust the agent. For T3, we gate the entire task.

---

## 5. Skills System

### Hermes: Three-Tier Progressive Disclosure
1. System prompt: name + 60-char description per skill (always loaded)
2. `skill_view(name)`: full SKILL.md on demand
3. `skill_view(name, "references/file.md")`: linked reference files

- Auto-created by agent after complex tasks
- Security scan on every write (create/edit/patch)
- Skills Hub with quarantine → scan → install pipeline
- Conditional visibility: `fallback_for_toolsets`, `requires_toolsets`

### gstack: Generated-from-Source Skills
- `.tmpl` files with `{{PLACEHOLDER}}` markers
- Placeholders resolved from actual TypeScript source (commands.ts, snapshot.ts)
- CI fails if generated docs are stale
- Single source of truth: code IS the documentation

### Superpowers: Persuasion-Engineered Skills
- Iron Law pattern: "NO [BEHAVIOR] WITHOUT [PREREQUISITE] FIRST"
- Rationalization tables (excuse -> reality)
- SUBAGENT-STOP guards prevent meta-skill-checking inside subagents
- CSO (Claude Search Optimization): description = triggers only, never workflow

---

## 6. GitHub Issues — Cross-Repo Pain Points

### Critical Patterns (all 4 repos share these)

**Session/Memory Loss** (8+ issues):
- Paperclip #1845: no crash-recovery wakeup after server restart
- gstack #401: context compaction erases all learnings
- Hermes #3212: session truncated to 4 messages mid-conversation
- Superpowers #601: no learning accumulation across subagent tasks

**Unbounded Storage Growth** (6+ issues):
- Paperclip #1846: heartbeat_runs result_json has no TTL
- Paperclip #1770: hourly backups never pruned
- Hermes #3015: session JSON files never deleted
- All repos lack GC strategy

**Rate Limit / Transient Error Handling** (5+ issues):
- Paperclip #1861: agent session dies on 429 with no recovery
- Paperclip #1763: heartbeat run marked "completed" on 529 overloaded error
- Hermes #2962: OAuth refresh fails in headless gateway
- No repo has robust transient error → retry → fallback chain

**Subagent Budget/Control** (4+ issues):
- Hermes #2873: shared IterationBudget drains parent's quota
- Superpowers #716: too much time on reviews, subagent interactions
- gstack #497: autoplan subagents abandoned on "run simultaneously"
- Paperclip #1749: idle runs block concurrency slots indefinitely

**Provider-Specific Fragility** (10+ issues):
- Paperclip: OpenClaw adapter crashes on WebSocket close, Gemini costs not tracked
- Hermes: fallback_model ignores custom endpoint config, GitHub Copilot premium unused
- gstack: Codex symlinks broken, Gemini CLI not supported
- Superpowers: no subagent support on Gemini CLI

**Security** (3 critical):
- Paperclip #1818: GET /api/agents leaks plaintext secrets from adapter env vars
- gstack #545: supply chain concern (GenScript.AORA in Chrome extension)
- Hermes: prompt injection scanning in memory tool (good practice)

### Top Feature Requests Across Repos
1. Cross-session learnings / durable memory (superpowers #601, #907; gstack #401)
2. Multi-model collaboration (superpowers #730; gstack #350)
3. Better error recovery / model fallback (paperclip #1861; hermes #3124)
4. Budget controls beyond dollar amounts (paperclip #1756: token-based budgets)
5. Subagent permission control (hermes #2986)
6. Structured output format (hermes #3326: --output-format json)

---

## 7. Patterns to Adopt in Specpunk

| Pattern | Source | Priority | Effort |
|---------|--------|----------|--------|
| Frozen snapshot for session context | Hermes | P0 | Low |
| Receipt schema with version + validation | All (lesson from bugs) | P0 | Low |
| 4-status protocol (DONE/CONCERNS/BLOCKED/NEEDS_CONTEXT) | Superpowers | P0 | Low |
| Budget isolation per task (not shared) | Hermes bug #2873 | P0 | Already have |
| TTL on all persistent state | All (storage growth bugs) | P1 | Med |
| Atomic writes (temp + mv) everywhere | Hermes, gstack | P1 | Med |
| Transient error detection + retry + fallback | Paperclip #1861 | P1 | Med |
| Security: never leak adapter env vars | Paperclip #1818 | P1 | Low |
| Structured handoff markdown between sessions | Paperclip | P2 | Med |
| Skill progressive disclosure (index + on-demand) | Hermes | P2 | Low |
| Dangerous command patterns (22 regexes) | Hermes | P3 | Low |
| Unicode normalization before pattern matching | Hermes | P3 | Low |

## 8. Anti-Patterns to Avoid

| Anti-Pattern | Source | Why |
|-------------|--------|-----|
| Mark transient errors as "completed" | Paperclip #1763 | Silent data corruption |
| Shared iteration budget parent<->child | Hermes #2873 | Subagent starves parent |
| No TTL on DB/file storage | All repos | Unbounded disk growth |
| Idle runs blocking concurrency slots | Paperclip #1749 | Slot starvation |
| OAuth token refresh in headless = broken | Paperclip, Hermes | Use long-lived tokens |
| Symlink discovery across platforms | gstack #333 | Codex/Windows breaks |
| Hot reload without drain | Brainstorm Branch 2 | In-flight tasks corrupted |
