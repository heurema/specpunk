package check

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"regexp"
	"slices"
	"strings"
)

type Task struct {
	Task     string   `json:"task"`
	Allowed  []string `json:"allowed"`
	Changed  []string `json:"changed"`
	Blocked  []string `json:"blocked"`
	Evidence []string `json:"evidence"`
}

var multiSlashPattern = regexp.MustCompile(`/+`)

type Result struct {
	ScopeStatus    string
	Decision       string
	Reason         string
	InScope        []string
	OutOfScope     []string
	BlockedTouched []string
}

type GenerateOptions struct {
	ChangedManifestPath string
	ChangedDiffPath     string
	ChangedDiffReader   io.Reader
	ChangedGitRange     string
	WorkingDir          string
}

func GenerateFromFile(path string, options GenerateOptions) (string, error) {
	task, err := LoadTask(path)
	if err != nil {
		return "", err
	}

	switch {
	case options.ChangedManifestPath != "":
		changed, err := LoadChangedManifest(options.ChangedManifestPath)
		if err != nil {
			return "", err
		}
		task.Changed = changed
	case options.ChangedDiffPath == "-":
		if options.ChangedDiffReader == nil {
			return "", fmt.Errorf("stdin diff reader is required when --changed-diff is '-'")
		}
		changed, err := LoadChangedDiffReader(options.ChangedDiffReader, "stdin")
		if err != nil {
			return "", err
		}
		task.Changed = changed
	case options.ChangedDiffPath != "":
		changed, err := LoadChangedDiff(options.ChangedDiffPath)
		if err != nil {
			return "", err
		}
		task.Changed = changed
	case options.ChangedGitRange != "":
		changed, err := LoadChangedGitRange(options.WorkingDir, options.ChangedGitRange)
		if err != nil {
			return "", err
		}
		task.Changed = changed
	}

	if len(task.Changed) == 0 {
		return "", fmt.Errorf("changed files must be provided either in field 'changed' or via --changed-manifest/--changed-diff")
	}

	result := Classify(task)
	return RenderMarkdown(task, result), nil
}

func LoadTask(path string) (Task, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return Task{}, fmt.Errorf("task file not found: %s", path)
	}

	var task Task
	if err := json.Unmarshal(content, &task); err != nil {
		return Task{}, fmt.Errorf("task file is not valid JSON: %s", path)
	}

	task.Task = strings.TrimSpace(task.Task)
	if task.Task == "" {
		return Task{}, fmt.Errorf("field 'task' is required")
	}

	task.Allowed = normalizeList(task.Allowed, true)
	task.Changed = normalizeList(task.Changed, true)
	task.Blocked = normalizeList(task.Blocked, true)
	task.Evidence = normalizeList(task.Evidence, false)

	if len(task.Allowed) == 0 {
		return Task{}, fmt.Errorf("field 'allowed' must include at least one path or pattern")
	}
	return task, nil
}

func LoadChangedManifest(path string) ([]string, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("changed manifest file not found: %s", path)
	}

	trimmed := strings.TrimSpace(string(content))
	if trimmed == "" {
		return nil, fmt.Errorf("changed manifest is empty: %s", path)
	}

	if strings.HasPrefix(trimmed, "[") {
		var items []string
		if err := json.Unmarshal([]byte(trimmed), &items); err != nil {
			return nil, fmt.Errorf("changed manifest JSON is not valid: %s", path)
		}
		items = normalizeList(items, true)
		if len(items) == 0 {
			return nil, fmt.Errorf("changed manifest must include at least one changed file: %s", path)
		}
		return items, nil
	}

	scanner := bufio.NewScanner(strings.NewReader(trimmed))
	lines := make([]string, 0)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		lines = append(lines, line)
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("changed manifest could not be read: %s", path)
	}

	lines = normalizeList(lines, true)
	if len(lines) == 0 {
		return nil, fmt.Errorf("changed manifest must include at least one changed file: %s", path)
	}

	return lines, nil
}

func LoadChangedGitRange(workingDir string, revspec string) ([]string, error) {
	revspec = strings.TrimSpace(revspec)
	if revspec == "" {
		return nil, fmt.Errorf("git revspec is required")
	}

	cmd := exec.Command("git", "diff", "--name-only", "--relative", revspec)
	if workingDir != "" {
		cmd.Dir = workingDir
	}

	output, err := cmd.CombinedOutput()
	if err != nil {
		message := strings.TrimSpace(string(output))
		if message == "" {
			message = err.Error()
		}
		return nil, fmt.Errorf("git diff failed: %s", message)
	}

	lines := strings.Split(string(output), "\n")
	lines = normalizeList(lines, true)
	if len(lines) == 0 {
		return nil, fmt.Errorf("git diff returned no changed files for revspec %q", revspec)
	}

	return lines, nil
}

func LoadChangedDiff(path string) ([]string, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("changed diff file not found: %s", path)
	}
	defer file.Close()

	return LoadChangedDiffReader(file, path)
}

func LoadChangedDiffReader(reader io.Reader, source string) ([]string, error) {
	scanner := bufio.NewScanner(reader)
	items := make([]string, 0)
	expectingPlusPath := false
	sawLine := false

	for scanner.Scan() {
		sawLine = true
		line := scanner.Text()

		switch {
		case strings.HasPrefix(line, "diff --git "):
			expectingPlusPath = false
			path := parseGitDiffPath(strings.TrimPrefix(line, "diff --git "))
			if path != "" {
				items = append(items, path)
			}
		case strings.HasPrefix(line, "--- "):
			expectingPlusPath = true
		case expectingPlusPath && strings.HasPrefix(line, "+++ "):
			expectingPlusPath = false
			path := parsePatchHeaderPath(strings.TrimPrefix(line, "+++ "))
			if path != "" {
				items = append(items, path)
			}
		default:
			expectingPlusPath = false
		}
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("changed diff could not be read: %s", source)
	}
	if !sawLine {
		return nil, fmt.Errorf("changed diff is empty: %s", source)
	}

	items = normalizeList(items, true)
	if len(items) == 0 {
		return nil, fmt.Errorf("changed diff must include at least one changed file: %s", source)
	}

	return items, nil
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
	value = multiSlashPattern.ReplaceAllString(value, "/")
	return value
}

func parseGitDiffPath(value string) string {
	fields := strings.Fields(value)
	if len(fields) < 2 {
		return ""
	}
	return parsePatchHeaderPath(fields[1])
}

func parsePatchHeaderPath(value string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return ""
	}
	if index := strings.IndexByte(value, '\t'); index >= 0 {
		value = value[:index]
	}
	if value == "/dev/null" {
		return ""
	}
	value = strings.TrimPrefix(value, "a/")
	value = strings.TrimPrefix(value, "b/")
	return normalizePath(value)
}

func Classify(task Task) Result {
	blockedTouched := make([]string, 0)
	inScope := make([]string, 0)

	for _, changed := range task.Changed {
		if matchesAny(changed, task.Blocked) {
			blockedTouched = append(blockedTouched, changed)
			continue
		}
		if matchesAny(changed, task.Allowed) {
			inScope = append(inScope, changed)
		}
	}

	outOfScope := make([]string, 0)
	for _, changed := range task.Changed {
		if !slices.Contains(inScope, changed) {
			outOfScope = append(outOfScope, changed)
		}
	}

	scopeStatus := "respected"
	if len(outOfScope) > 0 {
		scopeStatus = "drifted"
	}

	decision := "approve"
	reason := "scope stayed bounded and evidence is attached"
	switch {
	case len(blockedTouched) > 0:
		decision = "inspect"
		reason = "blocked files were touched"
	case len(outOfScope) > 0:
		decision = "inspect"
		reason = "changed files exceeded the declared scope"
	case len(task.Evidence) == 0:
		decision = "inspect"
		reason = "scope stayed bounded but evidence is missing"
	}

	return Result{
		ScopeStatus:    scopeStatus,
		Decision:       decision,
		Reason:         reason,
		InScope:        inScope,
		OutOfScope:     outOfScope,
		BlockedTouched: blockedTouched,
	}
}

func matchesAny(path string, patterns []string) bool {
	for _, pattern := range patterns {
		if matchPattern(path, pattern) {
			return true
		}
	}
	return false
}

func matchPattern(path string, pattern string) bool {
	path = normalizePath(path)
	pattern = normalizePath(pattern)

	regex := regexp.QuoteMeta(pattern)
	regex = strings.ReplaceAll(regex, `\*\*`, `.*`)
	regex = strings.ReplaceAll(regex, `\*`, `.*`)
	regex = strings.ReplaceAll(regex, `\?`, `.`)

	matched, err := regexp.MatchString("^"+regex+"$", path)
	return err == nil && matched
}

func RenderMarkdown(task Task, result Result) string {
	return fmt.Sprintf(`# Generated Review Artifact

Task: %s
Decision: %s
Reason: %s

## Scope Summary

- Declared allowed patterns: %d
- Declared blocked patterns: %d
- Changed files: %d
- In scope: %d
- Out of scope: %d
- Blocked touched: %d
- Scope status: %s

## Allowed Patterns

%s

## Blocked Patterns

%s

## Changed Files

%s

## Out Of Scope Files

%s

## Evidence

%s

## Reviewer Posture

- %s
- %s
- %s
`,
		task.Task,
		result.Decision,
		result.Reason,
		len(task.Allowed),
		len(task.Blocked),
		len(task.Changed),
		len(result.InScope),
		len(result.OutOfScope),
		len(result.BlockedTouched),
		result.ScopeStatus,
		formatBullets(task.Allowed, "none"),
		formatBullets(task.Blocked, "none"),
		formatBullets(task.Changed, "none"),
		formatBullets(result.OutOfScope, "none"),
		formatBullets(task.Evidence, "none"),
		reviewerDecision(result.Decision),
		reviewerScope(result.ScopeStatus),
		reviewerEvidence(task.Evidence),
	)
}

func formatBullets(items []string, emptyLabel string) string {
	if len(items) == 0 {
		return "- " + emptyLabel
	}
	lines := make([]string, 0, len(items))
	for _, item := range items {
		lines = append(lines, fmt.Sprintf("- `%s`", item))
	}
	return strings.Join(lines, "\n")
}

func reviewerDecision(decision string) string {
	if decision == "approve" {
		return "approve the bounded change"
	}
	return "inspect the change before approval"
}

func reviewerScope(scopeStatus string) string {
	if scopeStatus == "respected" {
		return "scope stayed within the declared boundary"
	}
	return "scope drift is visible and must be understood"
}

func reviewerEvidence(evidence []string) string {
	if len(evidence) > 0 {
		return "evidence is attached to support the change"
	}
	return "evidence still needs to be attached"
}
