package taskdir

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type Input struct {
	Task     string   `json:"task"`
	Allowed  []string `json:"allowed"`
	Blocked  []string `json:"blocked"`
	Evidence []string `json:"evidence"`
}

type InitOptions struct {
	Task     string
	Allowed  []string
	Blocked  []string
	Evidence []string
	Now      time.Time
}

func Init(dir string, options InitOptions) error {
	dir = strings.TrimSpace(dir)
	if dir == "" {
		return fmt.Errorf("task directory is required")
	}

	input := Input{
		Task:     strings.TrimSpace(options.Task),
		Allowed:  normalizeList(options.Allowed, true),
		Blocked:  normalizeList(options.Blocked, true),
		Evidence: normalizeList(options.Evidence, false),
	}

	if input.Task == "" {
		return fmt.Errorf("task text is required")
	}
	if len(input.Allowed) == 0 {
		return fmt.Errorf("at least one allowed path or pattern is required")
	}

	now := options.Now
	if now.IsZero() {
		now = time.Now()
	}
	date := now.Format("2006-01-02")

	files := map[string]string{
		filepath.Join(dir, "input.json"):   renderInputJSON(input),
		filepath.Join(dir, "intent.md"):    renderIntent(date, input),
		filepath.Join(dir, "scope.md"):     renderScope(date, input),
		filepath.Join(dir, "evidence.md"):  renderEvidence(date, input),
		filepath.Join(dir, "review.md"):    renderReview(date, input),
	}

	for path := range files {
		if _, err := os.Stat(path); err == nil {
			return fmt.Errorf("refusing to overwrite existing file: %s", path)
		} else if !os.IsNotExist(err) {
			return err
		}
	}

	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}

	for path, content := range files {
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			return err
		}
	}

	return nil
}

func renderInputJSON(input Input) string {
	body, err := json.MarshalIndent(input, "", "  ")
	if err != nil {
		panic(err)
	}
	return string(body) + "\n"
}

func renderIntent(date string, input Input) string {
	var builder strings.Builder
	builder.WriteString("# Intent\n\n")
	builder.WriteString("Last updated: " + date + "\n")
	builder.WriteString("Task status: draft\n\n")
	builder.WriteString("## Change Intent\n\n")
	builder.WriteString(input.Task + "\n\n")
	builder.WriteString("## Must Preserve\n\n")
	builder.WriteString("- keep the change inside the declared scope\n")
	builder.WriteString("- attach evidence before approval\n\n")
	builder.WriteString("## Must Not Introduce\n\n")
	builder.WriteString("- unrelated file edits\n")
	builder.WriteString("- untracked behavior changes\n")
	return builder.String()
}

func renderScope(date string, input Input) string {
	var builder strings.Builder
	builder.WriteString("# Scope\n\n")
	builder.WriteString("Last updated: " + date + "\n")
	builder.WriteString("Task status: draft\n\n")
	builder.WriteString("## Allowed\n\n")
	builder.WriteString(renderList(input.Allowed))
	builder.WriteString("\n## Blocked\n\n")
	if len(input.Blocked) == 0 {
		builder.WriteString("- none declared yet\n")
	} else {
		builder.WriteString(renderList(input.Blocked))
	}
	builder.WriteString("\n## Review Rule\n\n")
	builder.WriteString("Any file outside the allowed set should force `inspect`.\n")
	return builder.String()
}

func renderEvidence(date string, input Input) string {
	var builder strings.Builder
	builder.WriteString("# Evidence\n\n")
	builder.WriteString("Last updated: " + date + "\n")
	builder.WriteString("Task status: draft\n\n")
	builder.WriteString("## Expected Evidence\n\n")
	if len(input.Evidence) == 0 {
		builder.WriteString("- add at least one evidence note before approval\n")
	} else {
		builder.WriteString(renderList(input.Evidence))
	}
	builder.WriteString("\n## Validation Notes\n\n")
	builder.WriteString("- record how the change was checked\n")
	builder.WriteString("- keep evidence shorter than the diff\n")
	return builder.String()
}

func renderReview(date string, input Input) string {
	var builder strings.Builder
	builder.WriteString("# Review\n\n")
	builder.WriteString("Last updated: " + date + "\n")
	builder.WriteString("Reviewer posture: pending\n")
	builder.WriteString("Task status: draft\n\n")
	builder.WriteString("## Decision\n\n")
	builder.WriteString("Pending.\n\n")
	builder.WriteString("## Why\n\n")
	builder.WriteString("- task initialized from `specpunk task init`\n")
	builder.WriteString("- run `specpunk check --task-dir <this-dir>` after changed files are known\n")
	builder.WriteString("\n## Task\n\n")
	builder.WriteString(input.Task + "\n")
	return builder.String()
}

func renderList(items []string) string {
	var builder strings.Builder
	for _, item := range items {
		builder.WriteString("- `" + item + "`\n")
	}
	return builder.String()
}

func normalizeList(items []string, pathLike bool) []string {
	seen := map[string]struct{}{}
	result := make([]string, 0, len(items))

	for _, item := range items {
		value := strings.TrimSpace(item)
		if value == "" {
			continue
		}
		if pathLike {
			value = normalizePath(value)
		}
		if _, ok := seen[value]; ok {
			continue
		}
		seen[value] = struct{}{}
		result = append(result, value)
	}

	return result
}

func normalizePath(value string) string {
	value = strings.ReplaceAll(value, "\\", "/")
	value = strings.TrimPrefix(value, "./")
	for strings.Contains(value, "//") {
		value = strings.ReplaceAll(value, "//", "/")
	}
	return value
}
