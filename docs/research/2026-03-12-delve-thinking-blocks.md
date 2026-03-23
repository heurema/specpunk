# Thinking Blocks as Lost Intent — Claude Code Session Forensics

**Date:** 2026-03-12
**Classification:** Deep Research
**Context:** specpunk — AI tooling analysis
**Status:** Complete

---

## Executive Summary

Claude Code writes every session to JSONL files at `~/.claude/projects/<encoded-path>/<uuid>.jsonl`. These files contain, among other things, the model's extended thinking blocks — raw internal monologues averaging 554 characters each, with peaks over 5,000 characters — that capture decision rationale, tradeoff analysis, implementation planning, and error recovery reasoning that is never shown to the user and never persisted in any structured form. Across 466 JSONL files totaling 783 MB on a single machine, thousands of thinking blocks represent an untapped corpus of engineering intent.

This document investigates the forensics of that corpus: what the schema looks like, what the thinking content contains, how to extract and structure it, how to intercept it before compaction, how to build a cross-session intent graph, what privacy risks arise, and how Claude Code compares to peers. The document concludes with a concrete specification for a tool called **Delve** that captures, indexes, and surfaces this reasoning.

---

## 1. Session JSONL Forensics

### 1.1 Storage Location and Volume

Claude Code writes session data to:
```
~/.claude/projects/<url-encoded-path>/<session-uuid>.jsonl
```

The path encoding replaces `/` with `-` and drops the leading slash. For a project at `/Users/vi/vicc`, the directory is `-Users-vi-vicc`. Subagents launched in background write to the same directory as their parent session.

Empirical measurements from a production machine:
- 466 JSONL files across all projects
- 783.7 MB total size
- Largest single session: 49 MB (a long agentic run with many tool calls)
- Files retained indefinitely; no automatic pruning

Sessions with thinking blocks specifically:
- 11 of 20 sampled vicc sessions (55%) contain thinking blocks
- Top session: 109,133 characters across 111 blocks
- Average block: 554 characters (~138 tokens)
- Median block: 317 characters
- Blocks > 1,000 characters: 52 in top-10 sessions

### 1.2 Complete Record Schema

Each JSONL file contains newline-delimited JSON records. There are six record types:

#### `file-history-snapshot`
Periodic snapshot of tracked file states. Written before and after edits.
```json
{
  "type": "file-history-snapshot",
  "messageId": "7db8ac75-add8-409a-a49f-e5b53e167e6f",
  "snapshot": {
    "messageId": "7db8ac75-...",
    "trackedFileBackups": {},
    "timestamp": "2026-03-10T16:16:15.546Z"
  },
  "isSnapshotUpdate": false
}
```

#### `progress`
Hook execution events and internal state transitions.
```json
{
  "type": "progress",
  "parentUuid": null,
  "isSidechain": false,
  "userType": "external",
  "cwd": "/Users/vi/vicc",
  "sessionId": "86dc693c-fc17-4d6c-86b5-336cbd66df0f",
  "version": "2.1.52",
  "gitBranch": "HEAD",
  "data": {
    "type": "hook_progress",
    "hookEvent": "SessionStart",
    "hookName": "SessionStart:clear",
    "command": "bd prime"
  },
  "parentToolUseID": "e817e8bb-9dba-43bf-a109-4f3c492793ff",
  "toolUseID": "e817e8bb-9dba-43bf-a109-4f3c492793ff",
  "timestamp": "2026-02-24T07:47:18.139Z",
  "uuid": "81a76b80-3fca-4dee-82f2-f87966a5c10e"
}
```

The `data.type` field distinguishes `hook_progress` from `mcp_progress` (MCP server lifecycle events with `serverName`, `toolName`, `elapsedTime`).

#### `user`
User messages and tool results. Content can be either a plain string or an array of content blocks.
```json
{
  "type": "user",
  "parentUuid": "a4732a0b-e804-4720-930a-5577aef08890",
  "isSidechain": false,
  "userType": "external",
  "cwd": "/Users/vi/vicc",
  "sessionId": "86dc693c-...",
  "version": "2.1.52",
  "gitBranch": "HEAD",
  "isMeta": false,
  "message": {
    "role": "user",
    "content": [
      {
        "type": "tool_result",
        "tool_use_id": "toolu_012zTe1ig4Cch2Vj17CgfLG4",
        "content": [{"type": "tool_reference", "tool_name": "Skill"}],
        "is_error": false
      }
    ]
  },
  "uuid": "ac6ffc68-e16b-4094-a057-1e35726e4186",
  "timestamp": "2026-02-24T07:48:29.298Z",
  "toolUseResult": {
    "matches": ["Skill"],
    "query": "select:Skill",
    "total_deferred_tools": 230
  },
  "sourceToolAssistantUUID": "a4732a0b-..."
}
```

The `toolUseResult` field contains structured data from tool execution — often richer than the tool_result content block.

#### `assistant`
The core record. Contains thinking blocks, text, and tool calls.
```json
{
  "type": "assistant",
  "parentUuid": "a8d276b6-9c92-45b9-b67d-5e0c7813019a",
  "isSidechain": false,
  "userType": "external",
  "cwd": "/Users/vi/vicc",
  "sessionId": "86dc693c-...",
  "version": "2.1.52",
  "gitBranch": "HEAD",
  "message": {
    "model": "claude-opus-4-6",
    "id": "msg_017B9mBfk82mNg86d5w5t5EQ",
    "type": "message",
    "role": "assistant",
    "content": [
      {
        "type": "thinking",
        "thinking": "The user wants me to launch a team...",
        "signature": "EqEGCkYICxgCKkC1+L49..."
      },
      {
        "type": "tool_use",
        "id": "toolu_012zTe1ig4Cch2Vj17CgfLG4",
        "name": "ToolSearch",
        "input": {"query": "select:Skill", "max_results": 1}
      }
    ],
    "stop_reason": null,
    "stop_sequence": null,
    "usage": {
      "input_tokens": 3,
      "cache_creation_input_tokens": 18878,
      "cache_read_input_tokens": 8457,
      "cache_creation": {
        "ephemeral_5m_input_tokens": 0,
        "ephemeral_1h_input_tokens": 18878
      },
      "output_tokens": 10,
      "service_tier": "standard",
      "inference_geo": "not_available"
    }
  },
  "requestId": "req_011CYSXTA3cTG1i8hNN25RxT",
  "uuid": "b69b6695-9f0c-4619-902c-ce58dfe5a2c0",
  "timestamp": "2026-02-24T07:48:29.238Z"
}
```

Content block types within assistant messages (from analysis of the top sessions):
- `thinking` — internal reasoning (111 per large session, avg 554 chars)
- `tool_use` — tool invocations (754 per large session)
- `text` — visible output (405 per large session)

#### `system`
Tool execution results and hook outcomes.
```json
{
  "type": "system",
  "subtype": "tool_result",
  "cwd": "/Users/vi/vicc",
  "sessionId": "86dc693c-...",
  "stopReason": "tool_use",
  "toolUseID": "...",
  "hookCount": 2,
  "hookInfos": [
    {"command": "...", "durationMs": 45}
  ],
  "hasOutput": true,
  "preventedContinuation": false,
  "level": "info",
  "slug": "composed-enchanting-platypus"
}
```

#### `queue-operation`
Background session management.
```json
{
  "type": "queue-operation",
  "operation": "push",
  "content": "...",
  "sessionId": "86dc693c-...",
  "timestamp": "2026-02-24T07:48:29.298Z"
}
```

### 1.3 Parent-Child Chain

Records are linked by `parentUuid`. An assistant record's `uuid` becomes the `parentUuid` of the user record that contains its tool results. This creates an explicit directed acyclic graph within each session file, enabling reconstruction of the full conversation tree including branching when multiple tools are called in parallel.

The `isSidechain: true` flag marks subagent messages — background agents launched via the `Task` tool write records to the parent's JSONL with this flag set.

### 1.4 The Thinking Block in Detail

The thinking block has three fields:
- `thinking` (string): The raw reasoning text, unformatted, stream-of-consciousness
- `signature` (string): A base64-encoded cryptographic signature generated by the API server
- `type` (literal "thinking")

The signature field is opaque and intentionally uninterpretable. Its purpose is to allow the API to verify, when thinking blocks are passed back in subsequent requests, that they were generated by Claude and have not been modified. Altering even one byte of the `thinking` text invalidates the signature and causes the API to reject the request with: *"thinking or redacted_thinking blocks in the latest assistant message cannot be modified."*

Source: [Anthropic Extended Thinking Docs](https://platform.claude.com/docs/en/build-with-claude/extended-thinking), [miteshashar/claude-code-thinking-blocks-fix](https://github.com/miteshashar/claude-code-thinking-blocks-fix)

A second block type, `redacted_thinking`, exists in the API but is **never persisted** to JSONL files. These are encrypted opaque blobs that exist only during active API calls. When Claude generates reasoning that triggers content policy filters, that reasoning is returned as a redacted block — visible to the system but not reconstructable from stored files. This is why some sessions show fewer thinking characters than expected.

For Claude Opus 4.6, the `budget_tokens` parameter for thinking is deprecated in favor of `adaptive` mode, which dynamically allocates thinking budget. For Sonnet 3.7, the full untruncated reasoning is returned; for Sonnet 4.6 and Opus 4.6, thinking content is *summarized* before storage — meaning what appears in the JSONL is a condensed version of the actual internal computation, with the full thinking tokens billed but not stored.

---

## 2. Thinking Block Content Analysis

### 2.1 Empirical Taxonomy

Analysis of the top-10 sessions by thinking volume (602 thinking blocks total, ~154K characters) produces this category distribution:

| Category | Count | Avg Size | Description |
|----------|-------|----------|-------------|
| skill_routing | 112 | 1,114 chars | Deciding which skill/hook/plugin to invoke |
| architectural | 56 | 1,058 chars | Design decisions, approach selection |
| tool_selection | 52 | 1,403 chars | Choosing which tool to use and how |
| error_recovery | 43 | 761 chars | Diagnosing failures, planning workarounds |
| wait_status | 39 | 198 chars | Monitoring background agents |
| implementation_planning | 26 | 282 chars | Sequencing steps, estimating work |
| risk_assessment | 1 | 136 chars | Security/safety considerations |
| other/boilerplate | 285 | 27 chars | Empty or near-empty blocks |

The "other" category at 285 blocks (47% by count) has only 27 chars average — these are the mechanically-generated empty thinking blocks produced when the model generates a response with minimal reasoning (e.g., simple acknowledgments, one-word answers). Filtering these out leaves ~317 substantive blocks.

### 2.2 Sample Thinking Block by Category

**Architectural (1,058 chars average):**
> "This is Microsoft's Amplifier project - a modular AI development assistant. It's a competitor/alternative to Claude Code with a modular architecture. The user wants a team to study it and discuss what can be borrowed for our current project. Let me think about the best approach here. I could launch parallel subagents to analyze different aspects: architecture, agent system, bundles/behaviors, documentation. For each researcher I need to define a clear scope with no overlap..."

This block contains:
1. Context synthesis (what is this thing)
2. Goal decomposition (what the user wants)
3. Strategy selection (parallel subagents vs sequential)
4. Task partitioning (scope definition to prevent overlap)

None of this appears in the visible response.

**Tool Selection (1,403 chars average):**
> "...I'm going back and forth on the best approach here. The shell expansion gets messy, so let me just pass the file paths directly to the CLI tools and let them handle reading the files themselves. For the Codex and Gemini commands, I'll structure simple prompts that reference the file locations rather than trying to embed the contents inline. I should verify that both `codex` and `gemini` are actually installed before trying to run them..."

This is internal deliberation about execution strategy — the model considering and rejecting approaches before committing. The user sees only the final execution, not the reasoning that led to it.

**Error Recovery (761 chars average):**
> "...I need to think about this differently. I assumed the file was at path X but the tool returned an error. Let me check if it's possible that the path changed, or if the file was moved. The working directory is /Users/vi/vicc but the error suggests it's trying /Users/vi. This is a path resolution issue — the hook is likely running with a different cwd than expected..."

This is diagnostic reasoning about unexpected tool behavior — the kind of reasoning a developer would want to capture in a postmortem but which is currently invisible.

### 2.3 The Novelty Distribution

Not all thinking is high-value. A rough breakdown by information value:

- **High-value** (~30%): Genuine architectural decisions, tradeoffs explicitly evaluated, security reasoning, approach selection with alternatives considered
- **Medium-value** (~25%): Implementation planning, error diagnosis, coordination reasoning for multi-agent tasks
- **Low-value/boilerplate** (~45%): Mechanical task decomposition, simple routing decisions, empty/near-empty blocks

The 30% high-value subset is what makes extraction worthwhile. In a 10MB session file with 111 thinking blocks, approximately 33 blocks contain reasoning that a developer would benefit from seeing post-session.

### 2.4 What's Missing

Despite the richness of thinking blocks, certain reasoning types are systematically absent:

1. **Confidence levels** — the model never explicitly states how confident it is in a decision within thinking blocks (it reasons through uncertainty but doesn't quantify it)
2. **Alternative paths taken** — thinking shows deliberation but doesn't always enumerate alternatives that were considered and rejected
3. **Cross-session references** — each thinking block is isolated to its turn; the model doesn't reference prior sessions or decisions in thinking blocks even when it would be logical to do so
4. **Performance reasoning** — thinking blocks rarely address token budget concerns, latency tradeoffs, or cost implications of tool choices

---

## 3. Extraction Pipeline Design

### 3.1 Pipeline Architecture

The extraction pipeline has three stages: parse, classify, extract.

```
~/.claude/projects/**/*.jsonl
          |
          v
    +-------------+
    |   Parser    |  Read JSONL, reconstruct turns, collect thinking blocks
    +-------------+
          |
          v
    +-------------+
    |  Classifier |  Filter noise, categorize by type, score novelty
    +-------------+
          |
          v
    +-------------+
    |  Extractor  |  LLM extraction -> structured Decision records
    +-------------+
          |
          v
    decisions.db (SQLite)
```

### 3.2 Parser

The parser reconstructs turn context from raw JSONL records. A "turn" is the unit of reasoning: one user input, one model response (which may include thinking + tool calls + text).

```python
from dataclasses import dataclass, field
from typing import Optional
import json

@dataclass
class ThinkingBlock:
    session_id: str
    turn_uuid: str
    timestamp: str
    thinking: str
    signature: str
    model: str
    cwd: str
    git_branch: str
    version: str
    input_tokens: int
    output_tokens: int
    cache_read_tokens: int
    # Adjacent context
    preceding_user_message: Optional[str] = None
    subsequent_tool_calls: list = field(default_factory=list)
    turn_text_output: Optional[str] = None
    is_sidechain: bool = False
    parent_uuid: Optional[str] = None

def parse_session(jsonl_path: str) -> list[ThinkingBlock]:
    """Parse a JSONL session file into ThinkingBlock records."""
    records = {}
    assistant_records = []

    with open(jsonl_path) as f:
        lines = f.readlines()

    # First pass: index all records
    for line in lines:
        try:
            r = json.loads(line)
            if r.get('uuid'):
                records[r['uuid']] = r
        except:
            pass

    # Second pass: collect assistant records
    for line in lines:
        try:
            r = json.loads(line)
            if r.get('type') == 'assistant':
                assistant_records.append(r)
        except:
            pass

    blocks = []
    for record in assistant_records:
        msg = record.get('message', {})
        content = msg.get('content', [])
        if not isinstance(content, list):
            continue

        usage = msg.get('usage', {})

        # Find preceding user message
        parent_uuid = record.get('parentUuid')
        preceding = None
        if parent_uuid and parent_uuid in records:
            parent_rec = records[parent_uuid]
            parent_content = parent_rec.get('message', {}).get('content', '')
            if isinstance(parent_content, str):
                preceding = parent_content

        # Collect from content blocks
        thinking_texts = []
        tool_names = []
        text_outputs = []

        for block in content:
            if not isinstance(block, dict):
                continue
            btype = block.get('type')
            if btype == 'thinking':
                thinking_texts.append(block.get('thinking', ''))
            elif btype == 'tool_use':
                tool_names.append(block.get('name', ''))
            elif btype == 'text':
                text_outputs.append(block.get('text', ''))

        for thinking_text in thinking_texts:
            if not thinking_text.strip():
                continue  # skip empty blocks

            tb = ThinkingBlock(
                session_id=record.get('sessionId', ''),
                turn_uuid=record.get('uuid', ''),
                timestamp=record.get('timestamp', ''),
                thinking=thinking_text,
                signature='',  # not needed for extraction
                model=msg.get('model', ''),
                cwd=record.get('cwd', ''),
                git_branch=record.get('gitBranch', ''),
                version=record.get('version', ''),
                input_tokens=usage.get('input_tokens', 0),
                output_tokens=usage.get('output_tokens', 0),
                cache_read_tokens=usage.get('cache_read_input_tokens', 0),
                preceding_user_message=preceding,
                subsequent_tool_calls=tool_names,
                turn_text_output=' '.join(text_outputs)[:500],
                is_sidechain=record.get('isSidechain', False),
                parent_uuid=parent_uuid,
            )
            blocks.append(tb)

    return blocks
```

### 3.3 Classifier

The classifier applies fast heuristic filters before expensive LLM extraction.

```python
import re

NOISE_PATTERNS = [
    r'^The (user|request) (wants|asks)',  # pure task restatement
    r'^(I\'ll|Let me|I will) (check|look|see)',  # mechanical next-step
    r'^(agents?|task) (are|is) (still )?running',  # wait-state monitoring
    r'^\s*$',  # empty
]

SIGNAL_PATTERNS = [
    (r'(trade.?off|vs\.?|versus|alternative|compare|option[s]?)', 'tradeoff', 2.0),
    (r'(architect|design|pattern|interface|contract|schema)', 'architectural', 1.8),
    (r'(risk|security|danger|warning|sensitive|careful)', 'risk_assessment', 2.5),
    (r'(error|fail|wrong|broken|issue|debug)', 'error_recovery', 1.5),
    (r'(approach|strategy|plan|sequence|order|first.*then)', 'implementation', 1.3),
    (r'(skill|hook|plugin|which (command|tool|method))', 'tool_routing', 1.2),
]

def score_block(tb: ThinkingBlock) -> tuple[float, str]:
    """
    Returns (signal_score, category).
    score 0.0 = noise, 1.0+ = extract.
    """
    text = tb.thinking.lower()

    # Hard filter: noise patterns
    for pattern in NOISE_PATTERNS:
        if re.search(pattern, text, re.IGNORECASE):
            return 0.0, 'noise'

    # Length filter: very short blocks are almost always mechanical
    if len(tb.thinking) < 100:
        return 0.1, 'boilerplate'

    # Score by signal patterns
    best_score = 0.5
    best_category = 'general'
    for pattern, category, multiplier in SIGNAL_PATTERNS:
        if re.search(pattern, text, re.IGNORECASE):
            score = min(1.0, 0.5 * multiplier)
            if score > best_score:
                best_score = score
                best_category = category

    # Boost for explicit deliberation markers
    deliberation_markers = ['however', 'but wait', 'actually', 'on the other hand', 'alternatively']
    if any(marker in text for marker in deliberation_markers):
        best_score = min(1.0, best_score + 0.2)

    # Boost for complexity (longer = more reasoning)
    if len(tb.thinking) > 1000:
        best_score = min(1.0, best_score + 0.15)

    return best_score, best_category
```

### 3.4 Extraction Schema

High-scoring blocks (>= 0.6) are sent to an LLM for structured extraction. The target schema:

```python
@dataclass
class Decision:
    # Identity
    id: str                          # SHA-256 of thinking text
    session_id: str
    timestamp: str
    category: str                    # architectural|tradeoff|error_recovery|implementation|risk

    # Content
    summary: str                     # 1-2 sentence summary
    rationale: str                   # the core reasoning, extracted
    alternatives_considered: list    # other approaches mentioned
    chosen_approach: str             # what was decided
    constraints: list                # constraints that shaped the decision
    open_questions: list             # unresolved questions noted in thinking

    # Context
    cwd: str
    git_branch: str
    model: str
    user_prompt_summary: str         # what triggered this reasoning
    tools_invoked: list              # tools called after this thinking

    # Graph linking
    contradicts: list                # IDs of decisions this contradicts
    refines: list                    # IDs of decisions this refines/extends
    related: list                    # semantically related decision IDs

    # Metadata
    confidence: float                # extractor's confidence in extraction quality
    source_block_len: int
    signal_score: float
```

### 3.5 Extraction Prompt

```
You are analyzing an AI assistant's internal reasoning block from a Claude Code session.
Extract structured decision information from the thinking block below.

Context:
- User prompt that triggered this: {user_prompt}
- Tools subsequently invoked: {tools}
- Working directory: {cwd}

Thinking block:
<thinking>
{thinking}
</thinking>

Extract the following as JSON. Be precise and conservative — only extract what is clearly stated.

{
  "summary": "1-2 sentence summary of the core decision or reasoning",
  "rationale": "the key reasoning that drove the decision, in 2-4 sentences",
  "alternatives_considered": ["list of alternative approaches mentioned or implied"],
  "chosen_approach": "what was ultimately decided or planned",
  "constraints": ["constraints that limited the decision space"],
  "open_questions": ["unresolved questions or uncertainties noted"],
  "category": "one of: architectural|tradeoff|error_recovery|implementation|risk|tool_routing|other",
  "confidence": 0.0-1.0
}

Return only valid JSON. If the thinking is mechanical (simple routing, status check) return {"skip": true}.
```

### 3.6 Recommended LLM for Extraction

Use `claude-haiku-4-6` for bulk extraction (cheap, fast, adequate for structured extraction from short texts). Use `claude-sonnet-4-6` for blocks with `signal_score > 0.85` where extraction quality matters.

Cost estimate: at ~550 chars average thinking + 400 chars context + 300 chars output = ~1,250 tokens per extraction call. At $0.25/M input tokens (Haiku), extracting 300 blocks costs ~$0.15. A full session library extraction is a one-time $2-5 cost.

---

## 4. Hook-Based Capture

### 4.1 PreCompact Hook

Claude Code fires `PreCompact` immediately before context compaction — either manual (`/compact`) or automatic (when the context window fills). This is the critical interception point before thinking content becomes summarized and compressed.

The hook receives this JSON on stdin:
```json
{
  "session_id": "abc123",
  "transcript_path": "/Users/vi/.claude/projects/.../<session-uuid>.jsonl",
  "cwd": "/Users/vi/vicc",
  "permission_mode": "default",
  "hook_event_name": "PreCompact",
  "trigger": "auto",
  "custom_instructions": ""
}
```

The `transcript_path` is the live JSONL file for the current session. Note: there is a known bug ([issue #8564](https://github.com/anthropics/claude-code/issues/8564)) where the Stop hook's `transcript_path` is stale. The PreCompact path is generally accurate.

PreCompact hooks only support `type: "command"` — not `type: "prompt"` or `type: "agent"`. This limits what can be done synchronously, but a command hook can write to a file or queue for async processing.

### 4.2 Intent Capture Hook

A PreCompact hook that extracts and archives thinking blocks before they're compacted away:

```python
#!/usr/bin/env python3
"""
PreCompact hook: extract thinking blocks before compaction.
Writes structured intent to ~/.local/share/delve/intents/<session>.jsonl
"""

import json
import sys
import os
import hashlib
from datetime import datetime, timezone

def read_hook_input():
    return json.load(sys.stdin)

def extract_thinking_blocks(transcript_path: str) -> list[dict]:
    """Read JSONL and extract thinking blocks with context."""
    blocks = []
    records = {}

    with open(transcript_path) as f:
        lines = f.readlines()

    # First pass: index all records
    for line in lines:
        try:
            r = json.loads(line)
            if r.get('uuid'):
                records[r['uuid']] = r
        except:
            pass

    # Second pass: find thinking blocks
    for line in lines:
        try:
            r = json.loads(line)
            if r.get('type') != 'assistant':
                continue

            content = r.get('message', {}).get('content', [])
            if not isinstance(content, list):
                continue

            # Get preceding user message
            parent_uuid = r.get('parentUuid')
            user_text = ''
            if parent_uuid and parent_uuid in records:
                parent_msg = records[parent_uuid].get('message', {}).get('content', '')
                if isinstance(parent_msg, str):
                    user_text = parent_msg[:200]

            for block in content:
                if isinstance(block, dict) and block.get('type') == 'thinking':
                    thinking = block.get('thinking', '')
                    if len(thinking) < 100:  # skip noise
                        continue

                    blocks.append({
                        'id': hashlib.sha256(thinking.encode()).hexdigest()[:16],
                        'session_id': r.get('sessionId'),
                        'turn_uuid': r.get('uuid'),
                        'timestamp': r.get('timestamp'),
                        'thinking': thinking,
                        'model': r.get('message', {}).get('model'),
                        'cwd': r.get('cwd'),
                        'git_branch': r.get('gitBranch'),
                        'user_prompt': user_text,
                        'captured_at': datetime.now(timezone.utc).isoformat(),
                    })
        except:
            pass

    return blocks

def main():
    hook_input = read_hook_input()
    transcript_path = hook_input.get('transcript_path', '')
    session_id = hook_input.get('session_id', 'unknown')

    if not transcript_path or not os.path.exists(transcript_path):
        sys.exit(0)  # non-blocking failure

    blocks = extract_thinking_blocks(transcript_path)
    if not blocks:
        sys.exit(0)

    # Write to archive
    archive_dir = os.path.expanduser('~/.local/share/delve/intents')
    os.makedirs(archive_dir, exist_ok=True)
    archive_path = os.path.join(archive_dir, f'{session_id}.jsonl')

    with open(archive_path, 'a') as f:
        for block in blocks:
            f.write(json.dumps(block) + '\n')

    sys.stderr.write(f'[delve] Captured {len(blocks)} thinking blocks before compaction\n')
    sys.exit(0)

if __name__ == '__main__':
    main()
```

Configuration in `~/.claude/settings.json`:
```json
{
  "hooks": {
    "PreCompact": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "python3 ~/.claude/hooks/intent-capture.py"
          }
        ]
      }
    ],
    "Stop": [
      {
        "matcher": "",
        "hooks": [
          {
            "type": "command",
            "command": "python3 ~/.claude/hooks/intent-capture.py"
          }
        ]
      }
    ]
  }
}
```

### 4.3 Technical Constraints

1. **PreCompact is command-only**: Cannot use agent or prompt hook types. Async processing must be queued.
2. **Summarized thinking on newer models**: Claude Opus 4.6 and Sonnet 4.6 return summarized thinking blocks, not the full internal computation. The stored thinking is a digest, not the raw reasoning chain.
3. **redacted_thinking never stored**: Thinking content that triggers content policy is replaced with opaque redacted blocks containing no extractable text. This affects potentially 5-15% of thinking blocks in sensitive domains.
4. **Signature invalidation**: If you modify thinking block text for any reason (e.g., redaction of sensitive content), the session becomes non-resumable. Store thinking separately from the original JSONL.
5. **isSidechain blocks**: Subagent thinking blocks have `isSidechain: true`. They're stored in the parent session file but represent a different agent's reasoning. Track them separately.
6. **Stop hook staleness bug**: The Stop hook's `transcript_path` may point to an outdated file ([issue #8564](https://github.com/anthropics/claude-code/issues/8564)). Workaround: find the most recently modified `.jsonl` in the project directory.

---

## 5. Cross-Session Intent Graph

### 5.1 Graph Schema

A graph of engineering intent across sessions has the following node and edge types:

```
Nodes:
  Decision {id, summary, category, timestamp, session_id, cwd}
  Session  {id, start_time, end_time, project_path, model, turn_count}
  Project  {path, canonical_name}
  File     {path, last_touched}

Edges:
  Decision --MADE_IN--> Session
  Session  --IN--> Project
  Decision --CONTRADICTS--> Decision  {detected_by: "llm|rule"}
  Decision --REFINES--> Decision
  Decision --RELATED--> Decision      {similarity: float}
  Decision --TOUCHES--> File
```

### 5.2 Contradiction Detection

The most valuable cross-session signal is detecting when a later decision contradicts an earlier one. This can happen when:
- The model revisits an architectural decision and reaches a different conclusion
- A tool selection strategy changes (e.g., "use bash for X" followed later by "never use bash for X")
- Error recovery reveals that a previous approach was wrong

Detection strategy:
1. **Embedding similarity**: Embed all decision summaries. Decisions with cosine similarity > 0.75 between their `chosen_approach` fields are contradiction candidates.
2. **LLM verification**: Send candidate pairs to a small LLM with the prompt: "Do these two decisions contradict each other? Decision A: {A.chosen_approach}. Decision B: {B.chosen_approach}. If yes, which is more recent and should be treated as authoritative? Return JSON."
3. **Rule-based**: Detect explicit contradiction markers ("actually", "we should NOT", "contrary to") in later decisions that mention concepts from earlier ones.

### 5.3 Intent Evolution Model

Track how a decision evolves over time by linking refining decisions:

```
D1 (2026-02-24): "Use parallel subagents for research tasks"
  |
  +-- REFINES --> D2 (2026-02-25): "Use parallel subagents but cap at 4 concurrent"
                    |
                    +-- REFINES --> D3 (2026-03-01): "4-agent cap, sequential for shared file state"
```

This chain forms the canonical current understanding: sequential for shared-state tasks, parallel capped at 4 for independent research.

### 5.4 Storage: SQLite with FTS5

```sql
CREATE TABLE decisions (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    project_path TEXT,
    timestamp TEXT NOT NULL,
    category TEXT NOT NULL,
    summary TEXT NOT NULL,
    rationale TEXT,
    chosen_approach TEXT,
    alternatives TEXT,  -- JSON array
    constraints TEXT,   -- JSON array
    open_questions TEXT, -- JSON array
    model TEXT,
    signal_score REAL,
    confidence REAL,
    source_thinking_len INTEGER,
    embedding BLOB      -- float32 vector, 1536-dim (optional)
);

CREATE VIRTUAL TABLE decisions_fts USING fts5(
    summary, rationale, chosen_approach, alternatives,
    content='decisions', content_rowid='rowid'
);

CREATE TABLE decision_edges (
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    edge_type TEXT NOT NULL,  -- contradicts|refines|related
    confidence REAL,
    detected_at TEXT,
    PRIMARY KEY (source_id, target_id, edge_type)
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    project_path TEXT,
    start_time TEXT,
    end_time TEXT,
    model TEXT,
    total_thinking_blocks INTEGER,
    total_thinking_chars INTEGER,
    decision_count INTEGER
);
```

### 5.5 Contradiction Handling Policy

When a contradiction is detected between D_old and D_new (D_new is more recent):

1. Both decisions are retained in the database (history is immutable)
2. A `CONTRADICTS` edge is added from D_new to D_old with `confidence` score
3. D_old gets `superseded_by = D_new.id`
4. Query interface surfaces the most recent decision in a contradiction chain as authoritative
5. The contradiction is flagged for review ("This reverses your February 24 decision about X")

---

## 6. Privacy and Security Implications

### 6.1 What Thinking Blocks May Contain

Thinking blocks are unfiltered internal reasoning. In practice, they may include:

1. **Sensitive file paths**: Thinking frequently references actual file paths, including paths to credentials, private keys, configuration files with secrets
2. **Code snippets with credentials**: If the user is working near credential files, thinking may include credential text seen in tool outputs
3. **Business logic**: Architectural decisions about proprietary systems are fully captured
4. **Personal information**: Reasoning about user-provided context that may include PII
5. **Security vulnerabilities**: Error recovery thinking may describe security issues before they're patched
6. **API keys and tokens**: If a tool returned output containing a key, the thinking analyzing that output may reproduce it

### 6.2 Known Attack Vectors

**Prompt injection via thinking**: A malicious file read by the agent could inject instructions into the context that appear in thinking blocks. These instructions would then be extracted and stored in the decision graph, potentially poisoning future queries.

**Stored secrets exfiltration**: The intent database becomes a secondary corpus of sensitive information. If the database file is readable by processes other than the extraction tool, secrets in thinking blocks could be exfiltrated.

**Cross-session information leakage**: The intent graph connects reasoning across projects. A decision extracted from project A might reference details of project B if the same session spanned multiple directories.

**Social engineering via intent**: The intent graph represents the developer's actual decision-making patterns, not their stated ones. This is a high-value target for social engineering attacks.

Source: [Researcher Uncovers 30+ Flaws in AI Coding Tools](https://thehackernews.com/2025/12/researchers-uncover-30-flaws-in-ai.html)

### 6.3 Mitigation Controls

**Secret scanning on extraction**:
```python
import re

SECRET_PATTERNS = [
    r'(sk-[a-zA-Z0-9]{20,})',           # OpenAI API keys
    r'(ghp_[a-zA-Z0-9]{36})',            # GitHub PAT
    r'(AKIA[0-9A-Z]{16})',               # AWS access key
    r'([a-f0-9]{64})',                   # 64-char hex (many key formats)
    r'(-----BEGIN [A-Z]+ PRIVATE KEY)', # PEM private key
    r'(password\s*[=:]\s*\S+)',          # plaintext passwords
]

def sanitize_thinking(thinking: str) -> tuple[str, list[str]]:
    """
    Replace secrets with [REDACTED] placeholders.
    Returns (sanitized_text, list_of_redaction_types).
    """
    redacted = []
    text = thinking
    for pattern in SECRET_PATTERNS:
        matches = re.findall(pattern, text, re.IGNORECASE)
        if matches:
            text = re.sub(pattern, '[REDACTED]', text, flags=re.IGNORECASE)
            redacted.append(pattern[:20])
    return text, redacted
```

**Database access control**: chmod 600 on `decisions.db`. Consider SQLCipher encryption for sensitive environments.

**Retention policy**: Thinking blocks containing secrets should not be stored at all — fail-closed on any secret detection.

**No cloud upload**: The intent database should never be uploaded to any cloud service without explicit user consent and end-to-end encryption.

### 6.4 Sensitive Domain Classification

Before extracting decisions, classify whether the thinking block is from a sensitive domain:
- Authentication/authorization code: elevated risk
- Cryptography: elevated risk
- Financial logic: elevated risk
- Network/API communication: moderate risk
- UI/frontend code: low risk
- Documentation: low risk

High-risk blocks should be reviewed by the user before being stored, not automatically extracted.

---

## 7. Comparison with Other AI Tools

### 7.1 Claude Code (Anthropic)

**Storage format**: JSONL, one record per streaming chunk
**Thinking storage**: YES — thinking blocks fully stored in JSONL with signature
**Location**: `~/.claude/projects/<encoded-path>/<uuid>.jsonl`
**Thinking content on new models**: SUMMARIZED (Opus 4.6, Sonnet 4.6 return condensed thinking)
**Access**: Raw file read (no API needed)
**Retention**: Indefinite, no pruning
**Extraction difficulty**: Low (open format, well-documented)

### 7.2 Codex CLI (OpenAI)

**Storage format**: JSONL, one record per event
**Thinking storage**: YES but ENCRYPTED — `response_item` records with `type: "reasoning"` contain `encrypted_content` (Fernet-encrypted ciphertext) rather than plaintext reasoning
**Location**: `~/.codex/sessions/YYYY/MM/DD/<name-uuid>.jsonl`
**Schema**:
```json
{
  "type": "response_item",
  "payload": {
    "type": "reasoning",
    "summary": [],
    "content": null,
    "encrypted_content": "gAAAAABpo-G2swYAf0bqdo6Z..."
  }
}
```
**Access**: Cannot extract reasoning without the decryption key (not user-accessible)
**Retention**: Indefinite
**Extraction difficulty**: Impossible for reasoning blocks; agent messages and tool calls are plaintext

The `summary` field is always an empty array in observed sessions. The `content` is always null. Only `encrypted_content` is populated, and it's encrypted with a key managed by the Codex CLI process. This is a deliberate design choice: OpenAI's o-series models' reasoning chains are treated as proprietary.

### 7.3 Gemini CLI (Google)

**Storage format**: JSON (monolithic; transitioning to JSONL per [issue #15292](https://github.com/google-gemini/gemini-cli/issues/15292))
**Thinking storage**: NO — session files do not contain separate thinking blocks
**Location**: `~/.gemini/tmp/<project_hash>/chats/session-<timestamp>-<id>.json`
**Schema**:
```json
{
  "sessionId": "...",
  "projectHash": "...",
  "startTime": "...",
  "lastUpdated": "...",
  "messages": [
    {
      "id": "...",
      "timestamp": "...",
      "type": "gemini",
      "content": "Full response text..."
    }
  ]
}
```
The `content` field for gemini-type messages is a plain string containing only the final visible response. Tool calls are stored as additional entries. No internal reasoning is separately captured.

**Retention**: 30 days by default, configurable

### 7.4 Cursor (Anysphere)

**Storage format**: SQLite (`state.vscdb`, `cursorDiskKV` table)
**Thinking storage**: Partial — a `thinking` field exists on message objects per community tooling analysis
**Location**: `~/Library/Application Support/Cursor/User/workspaceStorage/<hash>/state.vscdb`
**Key formats**: `composer.composerData`, `bubbleId:<composerId>:<bubbleId>`
**Agent sub-conversations**: stored as `.jsonl` in `agent-transcripts/` subdirectories
**Extraction difficulty**: High — requires SQLite access, undocumented schema, changes between versions

Active community tooling: [cursor-history](https://github.com/S2thend/cursor-history), [cursor-view](https://github.com/saharmor/cursor-view), [cursor-db-explorer MCP](https://www.pulsemcp.com/servers/jbdamask-cursor-db-explorer)

Cursor has also proposed the [Agent Trace specification](https://www.infoq.com/news/2026/02/agent-trace-cursor/) — a JSON format for attributing code changes to AI conversations, tracking outcomes rather than reasoning.

### 7.5 GitHub Copilot

**Storage format**: No local session files; all reasoning server-side
**Thinking storage**: NO — Copilot does not expose internal reasoning to the local filesystem
**Access**: Not possible via local file inspection

### 7.6 Comparison Table

| Tool | Reasoning Stored | Format | Accessible | Plaintext |
|------|-----------------|--------|-----------|-----------|
| Claude Code | YES (summarized on new models) | JSONL | Easy | YES |
| Codex CLI | YES | JSONL | Impossible | NO (encrypted) |
| Gemini CLI | NO (not as distinct blocks) | JSON | N/A | N/A |
| Cursor | Partial | SQLite | Hard | YES (with tools) |
| GitHub Copilot | NO | N/A | N/A | N/A |

**Claude Code is uniquely positioned**: It stores more structured reasoning than any other major AI coding assistant, in a plaintext format that is directly accessible without any special tooling. This is both its greatest advantage for intent extraction and its greatest privacy risk.

---

## 8. Buildable Tool Specification: Delve

### 8.1 Overview

**Delve** is a CLI tool and daemon that extracts, indexes, and surfaces engineering intent from Claude Code (and other AI tool) session files. The name comes from the act of going deeper into the actual reasoning behind AI-assisted decisions.

**Design goals**:
- Zero dependency on Claude Code — reads files directly, no API calls to Anthropic
- Privacy-first — local only, no telemetry, encryption by default
- Query interface matches developer mental models ("why did we decide X?", "what changed about Y?")
- Hooks into Claude Code lifecycle for real-time capture
- Minimal viable implementation in Python stdlib + SQLite

### 8.2 Architecture

```
+-------------------------------------------------------------+
|                         Delve                               |
|                                                             |
|  +--------------+    +--------------+    +---------------+  |
|  |  Watcher     |    |  Extractor   |    |   Index       |  |
|  |              |    |              |    |               |  |
|  | inotify/FSE  +--->| Parser       +--->| SQLite + FTS5 |  |
|  | hook output  |    | Classifier   |    | + embeddings  |  |
|  |              |    | LLM extract  |    |               |  |
|  +--------------+    +--------------+    +---------------+  |
|         |                                        |           |
|         |              +--------------------------+          |
|         |              v                                     |
|  +----------------------------------------------+           |
|  |                  CLI                          |           |
|  |  delve query "why did we pick SQLite"         |           |
|  |  delve decisions --session <id>               |           |
|  |  delve contradictions                         |           |
|  |  delve timeline --project /path/to/project    |           |
|  +----------------------------------------------+           |
+-------------------------------------------------------------+
```

### 8.3 Directory Structure

```
~/.local/share/delve/
├── delve.db              # SQLite database (encrypted)
├── intents/              # Raw thinking blocks captured by PreCompact hook
│   ├── <session-id>.jsonl
│   └── ...
├── extractions/          # LLM extraction outputs (cached)
│   ├── <decision-id>.json
│   └── ...
├── config.toml           # User configuration
└── delve.log             # Operation log
```

### 8.4 CLI Interface

```bash
# Query natural language
delve query "why did we decide to use parallel agents"
delve query "what are the constraints on the hook system"
delve query "how did our approach to error handling evolve"

# Browse decisions
delve decisions                          # all recent decisions
delve decisions --session <uuid>         # decisions from specific session
delve decisions --category architectural # filter by category
delve decisions --project ~/vicc        # decisions for a project
delve decisions --since 2026-03-01      # time-bounded

# View a decision
delve show <decision-id>                # full decision details with lineage

# Find contradictions
delve contradictions                    # all detected contradictions
delve contradictions --unresolved       # only unresolved ones

# Intent timeline
delve timeline --project ~/vicc         # how intent evolved over time
delve timeline --topic "agent strategy" # topic-specific timeline

# Import new sessions
delve import ~/.claude/projects/-Users-vi-vicc/   # import all sessions
delve import --session <uuid>                      # single session
delve import --watch                               # daemon mode

# Stats
delve stats                             # decision counts, coverage
```

### 8.5 Query Engine

Natural language queries use a hybrid approach:
1. **FTS5 keyword search** on summary + rationale + chosen_approach (fast, no LLM)
2. **Embedding similarity** for semantic queries (requires local embedding model or API call)
3. **LLM reranking** for complex multi-criteria queries

Query type detection:
```python
def classify_query(query: str) -> str:
    if len(query.split()) <= 3 and not query.startswith('how'):
        return 'fts'
    if any(w in query.lower() for w in ['evolve', 'changed', 'history', 'before', 'after']):
        return 'temporal'
    if any(w in query.lower() for w in ['contradict', 'reversed', 'changed mind']):
        return 'contradiction'
    return 'semantic'
```

### 8.6 Integration Points

**Claude Code PreCompact + Stop hooks** (primary, real-time):
```json
{
  "hooks": {
    "PreCompact": [{"hooks": [{"type": "command", "command": "delve capture --from-stdin"}]}],
    "Stop": [{"hooks": [{"type": "command", "command": "delve capture --from-stdin"}]}]
  }
}
```

**Batch import** (historical sessions):
```bash
delve import ~/.claude/projects/ --recursive
```

**CLAUDE.md injection** (surface relevant decisions in Claude's context):
```bash
# SessionStart hook injects recent decisions:
delve relevant --project "$PWD" --format markdown --limit 5
```

**MCP server** (optional, for Claude Code to query Delve directly during a session):
```json
{
  "mcpServers": {
    "delve": {
      "type": "stdio",
      "command": "delve",
      "args": ["mcp"]
    }
  }
}
```

### 8.7 Implementation Phases

**Phase 0 — Bootstrap (1 day)**:
- Parse JSONL, extract thinking blocks to flat JSONL archive
- PreCompact + Stop hook installation
- SQLite schema creation

**Phase 1 — Classification (1 day)**:
- Heuristic classifier (no LLM)
- FTS5 keyword search
- Basic CLI: `delve decisions`, `delve show`

**Phase 2 — Extraction (2 days)**:
- LLM extraction pipeline (Haiku for bulk, Sonnet for high-signal)
- Contradiction detection (embedding + LLM verification)
- `delve query`, `delve contradictions`

**Phase 3 — Graph (2 days)**:
- Cross-session linking
- Timeline view
- `delve timeline`

**Phase 4 — Integration (1 day)**:
- CLAUDE.md injection hook
- MCP server for in-session access
- `delve relevant` command

**Phase 5 — Privacy (ongoing)**:
- Secret scanning on all extraction
- Database encryption
- Retention policy

### 8.8 Language and Dependencies

Language: Python 3.11+
Dependencies:
- `sqlite3` (stdlib) — database
- `hashlib`, `json`, `re` (stdlib) — parsing and classification
- `anthropic` (pip) — LLM extraction (optional, degrades gracefully to heuristic-only)
- `sentence-transformers` (pip, optional) — local embeddings for semantic search

The tool functions without any pip dependencies in heuristic-only mode (Phases 0-1). LLM and embedding features are opt-in.

### 8.9 Configuration Schema

```toml
[database]
path = "~/.local/share/delve/delve.db"
encrypt = true

[extraction]
enabled = true
model = "claude-haiku-4-6"
high_signal_model = "claude-sonnet-4-6"
min_score_for_extraction = 0.6
min_thinking_length = 100

[embeddings]
enabled = false
model = "local"  # "local" or "openai" or "anthropic"

[capture]
sources = [
  "~/.claude/projects/",
  "~/.codex/sessions/"
]
watch = false

[privacy]
secret_scanning = true
fail_closed = true   # skip block if secret detected
retention_days = 0   # 0 = never delete

[output]
format = "markdown"
max_results = 10
```

---

## 9. Open Research Questions

### 9.1 Summarization Fidelity

Claude Opus 4.6 and Sonnet 4.6 return *summarized* thinking blocks. The question is: how much information is lost in the summarization? Is the summarized thinking a faithful digest of the full internal computation, or does it selectively compress certain reasoning types? This is not publicly documented by Anthropic.

An empirical approach: compare full-thinking sessions (Sonnet 3.7 or earlier Opus versions) against summarized-thinking sessions for equivalent tasks and measure decision extraction quality.

### 9.2 Thinking Block Alignment

Are thinking blocks causally responsible for the subsequent behavior, or are they post-hoc rationalizations? Research on chain-of-thought faithfulness ([Breaking the Chain, OpenReview 2025](https://openreview.net/forum?id=yfqHr7l2tG)) suggests that intermediate reasoning structures are sometimes treated as context rather than true causal mediators. If thinking blocks are post-hoc, extracting them as "decisions" is potentially misleading.

### 9.3 Cross-Model Portability

The intent graph built from Claude Code sessions represents reasoning from Claude models. If a team switches to Codex or Gemini for some tasks, the intent graph becomes incomplete. The schema should accommodate null-source decisions (inferred from outcomes rather than captured from reasoning).

### 9.4 Compaction Loss Rate

When Claude Code auto-compacts, the compaction summary replaces older portions of the conversation. What fraction of thinking blocks are lost at compaction vs. preserved in the summary? This determines the urgency of the PreCompact capture hook.

---

## 10. Sources

### Official Documentation

- [Claude Code Hooks Reference](https://code.claude.com/docs/en/hooks) — hook events, schemas, PreCompact details
- [Claude Extended Thinking API](https://platform.claude.com/docs/en/build-with-claude/extended-thinking) — thinking block schema, signature field, budget_tokens
- [Claude Code SDK Sessions](https://docs.claude.com/en/docs/claude-code/sdk/sdk-sessions) — session file format
- [Anthropic: How to Configure Hooks](https://claude.com/blog/how-to-configure-hooks) — hook configuration guide
- [AWS Bedrock: Thinking Encryption](https://docs.aws.amazon.com/bedrock/latest/userguide/claude-messages-thinking-encryption.html) — signature and redacted_thinking details

### Claude Code Tooling (Community)

- [claude-replay](https://github.com/es617/claude-replay) — session HTML replays
- [claude-session-viewer](https://github.com/jtklinger/claude-session-viewer) — TUI session browser
- [claude-JSONL-browser](https://www.claude-hub.com/resource/github-cli-withLinda-claude-JSONL-browser-claude-JSONL-browser/) — browser for session files
- [claude-code-thinking-blocks-fix](https://github.com/miteshashar/claude-code-thinking-blocks-fix) — corruption fix, explains schema issues
- [4 tools for browsing session history](https://dev.to/gonewx/i-tested-4-tools-for-browsing-claude-code-session-history-17ie) — comparison
- [Permanent archive of AI conversations](https://www.cengizhan.com/p/building-a-permanent-archive-of-every)
- [precompact-hook (recovery summaries)](https://github.com/mvara-ai/precompact-hook)
- [claude-code-hooks-mastery](https://github.com/disler/claude-code-hooks-mastery)
- [Pre-compaction hook feature request](https://github.com/anthropics/claude-code/issues/15923)
- [Stop hook stale transcript bug](https://github.com/anthropics/claude-code/issues/8564)

### Codex CLI

- [Codex session transcripts issue](https://github.com/openai/codex/issues/2765)
- [Codex Memory System (DeepWiki)](https://deepwiki.com/openai/codex/3.7-memory-system)

### Gemini CLI

- [Gemini CLI Session Management](https://geminicli.com/docs/cli/session-management/)
- [JSONL migration issue](https://github.com/google-gemini/gemini-cli/issues/15292)
- [Session Storage Details](https://fossies.org/linux/gemini-cli/docs/cli/session-management.md)

### Cursor

- [cursor-history tool](https://github.com/S2thend/cursor-history)
- [Cursor Data Storage Structure](https://zread.ai/S2thend/cursor-history/6-cursor-data-storage-structure)
- [Agent Trace specification](https://www.infoq.com/news/2026/02/agent-trace-cursor/)
- [cursor-view](https://github.com/saharmor/cursor-view)

### Security Research

- [30+ flaws in AI coding tools enabling data theft](https://thehackernews.com/2025/12/researchers-uncover-30-flaws-in-ai.html)
- [AI Data Security (NSA/CISA)](https://media.defense.gov/2025/May/22/2003720601/-1/-1/0/CSI_AI_DATA_SECURITY.PDF)
- [AI Assistant Privacy Comparison 2025](https://cybernews.com/ai-tools/ai-assistants-privacy-and-security-comparisons/)
- [Agentic AI Security](https://www.obsidiansecurity.com/blog/agentic-ai-security)

### ADR and Decision Capture

- [adr.github.io](https://adr.github.io/) — ADR tools
- [MADR: Markdown ADR](https://github.com/adr/madr)
- [AI-generated ADRs](https://adolfi.dev/blog/ai-generated-adr/)
- [Claude ADR System Guide](https://gist.github.com/joshrotenberg/a3ffd160f161c98a61c739392e953764)

### Research Papers

- [LangurTrace: Forensic analysis of local LLM applications](https://www.sciencedirect.com/science/article/pii/S2666281725001271)
- [LLM Forensics using Invocation Log Analysis](https://dl.acm.org/doi/10.1145/3689217.3690616)
- [Knowledge Graphs for Explainable AI](https://pmc.ncbi.nlm.nih.gov/articles/PMC11316662/)
- [PARSE: LLM-driven schema optimization for entity extraction](https://arxiv.org/html/2510.08623v1)
- [Breaking the Chain: CoT faithfulness](https://openreview.net/forum?id=yfqHr7l2tG)
- [From LLMs to Knowledge Graphs: Production Systems 2025](https://medium.com/@claudiubranzan/from-llms-to-knowledge-graphs-building-production-ready-graph-systems-in-2025-2b4aff1ec99a)
- [Schema-Guided Reasoning](https://abdullin.com/schema-guided-reasoning/)

---

## Appendix A: PreCompact Hook Input — Full Field Reference

From the official hooks documentation:

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | Current session UUID |
| `transcript_path` | string | Path to live JSONL file |
| `cwd` | string | Current working directory |
| `permission_mode` | string | "default", "bypassPermissions", etc. |
| `hook_event_name` | string | "PreCompact" |
| `trigger` | string | "manual" or "auto" |
| `custom_instructions` | string | `/compact` argument, or empty for auto |

All hook events also receive: `session_id`, `transcript_path`, `cwd`, `permission_mode`, `hook_event_name`.

PreCompact supports **command hooks only** (no prompt or agent hook types).

---

## Appendix B: Thinking Block Volume Reference

From empirical measurement of a production Claude Code installation:

| Metric | Value |
|--------|-------|
| Total JSONL files | 466 |
| Total storage | 783.7 MB |
| Sessions with thinking blocks (sample) | 11/20 (55%) |
| Thinking blocks per rich session | 62-111 |
| Average block size | 554 chars (~138 tokens) |
| Median block size | 317 chars |
| Maximum block size | 5,781 chars |
| Blocks > 1,000 chars | ~52 per top-10 sessions |
| Signal fraction (non-noise) | ~53% |
| High-value fraction | ~30% |

Category distribution (by block count in top-10 sessions):
- Noise/boilerplate: 47%
- Skill/tool routing: 18%
- Architectural: 9%
- Tool selection: 8%
- Error recovery: 7%
- Wait-state monitoring: 6%
- Implementation planning: 4%
- Risk assessment: <1%

---

## Appendix C: Codex Reasoning Schema

Codex CLI stores reasoning in `response_item` records with `type: "reasoning"`:

```json
{
  "timestamp": "2026-03-01T06:50:30.629Z",
  "type": "response_item",
  "payload": {
    "type": "reasoning",
    "summary": [],
    "content": null,
    "encrypted_content": "gAAAAABpo-G2swYAf0bqdo6ZvotvvoxjzYh2zZ0QtVR9s-..."
  }
}
```

The `encrypted_content` is a Fernet-encrypted string (Python cryptography library format). The encryption key is managed by the Codex CLI process and is not accessible to users or third-party tools.

The immediately following `event_msg` with `type: "agent_message"` contains the plaintext assistant response — extractable, but it's the post-reasoning output, not the reasoning itself:

```json
{
  "type": "event_msg",
  "payload": {
    "type": "agent_message",
    "message": "I'll add two new tasks..."
  }
}
```

---

## Appendix D: Gemini CLI Session Schema

Gemini CLI stores sessions as monolithic JSON files at `~/.gemini/tmp/<project_hash>/chats/`:

```json
{
  "sessionId": "917beee1-4e51-4158-b4f5-fc23caab06f0",
  "projectHash": "09b993501a9a33f8aff4227a2a7058483d62baa44f1ee9474ae3485b8fefc26e",
  "startTime": "2026-03-06T15:49:42.456Z",
  "lastUpdated": "2026-03-06T15:50:19.290Z",
  "messages": [
    {
      "id": "48ca0092-...",
      "timestamp": "2026-03-06T15:49:42.456Z",
      "type": "user",
      "content": [{"text": "...user prompt..."}]
    },
    {
      "id": "0cbde71d-...",
      "timestamp": "2026-03-06T15:50:19.289Z",
      "type": "gemini",
      "content": "Full response text (no separate thinking blocks)..."
    }
  ]
}
```

No distinct thinking blocks. All reasoning is implicitly part of the response generation but not surfaced in the session file. Transitioning to JSONL format per [github.com/google-gemini/gemini-cli/issues/15292](https://github.com/google-gemini/gemini-cli/issues/15292).

---

*Research complete. Delve specification ready for implementation planning.*
