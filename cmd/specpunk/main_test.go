package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRunCheckUsesDiffFromStdin(t *testing.T) {
	tempDir := t.TempDir()
	taskPath := filepath.Join(tempDir, "task.json")
	taskJSON := `{
  "task": "bounded stdin diff change",
  "allowed": ["site/index.html", "site/style.css"],
  "blocked": ["docs/research/**"],
  "evidence": ["manual sanity check"]
}`
	if err := os.WriteFile(taskPath, []byte(taskJSON), 0o644); err != nil {
		t.Fatalf("write task: %v", err)
	}

	diffBody := `diff --git a/site/index.html b/site/index.html
index 1111111..2222222 100644
--- a/site/index.html
+++ b/site/index.html
@@ -1 +1 @@
-before
+after
diff --git a/site/style.css b/site/style.css
index 3333333..4444444 100644
--- a/site/style.css
+++ b/site/style.css
@@ -1 +1 @@
-before
+after
`

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := run(
		[]string{"check", "--task", taskPath, "--changed-diff", "-"},
		strings.NewReader(diffBody),
		&stdout,
		&stderr,
	)

	if exitCode != 0 {
		t.Fatalf("expected exit code 0, got %d, stderr=%s", exitCode, stderr.String())
	}
	if !strings.Contains(stdout.String(), "Decision: approve") {
		t.Fatalf("expected approve decision, got:\n%s", stdout.String())
	}
	if stderr.Len() != 0 {
		t.Fatalf("expected empty stderr, got: %s", stderr.String())
	}
}

func TestRunCheckRejectsMultipleChangedSources(t *testing.T) {
	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := run(
		[]string{"check", "--task", "task.json", "--changed-diff", "-", "--changed-git", "HEAD~1..HEAD"},
		strings.NewReader(""),
		&stdout,
		&stderr,
	)

	if exitCode != 2 {
		t.Fatalf("expected exit code 2, got %d", exitCode)
	}
	if !strings.Contains(stderr.String(), "use only one changed-file source") {
		t.Fatalf("unexpected stderr: %s", stderr.String())
	}
}

func TestRunTaskInitCreatesScaffold(t *testing.T) {
	tempDir := t.TempDir()
	taskDir := filepath.Join(tempDir, "task")

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := run(
		[]string{
			"task", "init",
			"--task-dir", taskDir,
			"--task", "Review a bounded site change.",
			"--allow", "site/index.html",
			"--allow", "./site/style.css",
			"--block", "docs/research/**",
			"--evidence", "manual browser check",
		},
		strings.NewReader(""),
		&stdout,
		&stderr,
	)

	if exitCode != 0 {
		t.Fatalf("expected exit code 0, got %d, stderr=%s", exitCode, stderr.String())
	}

	for _, name := range []string{"input.json", "intent.md", "scope.md", "evidence.md", "review.md"} {
		if _, err := os.Stat(filepath.Join(taskDir, name)); err != nil {
			t.Fatalf("expected %s to exist: %v", name, err)
		}
	}

	if !strings.Contains(stdout.String(), "created task scaffold") {
		t.Fatalf("unexpected stdout: %s", stdout.String())
	}
	if stderr.Len() != 0 {
		t.Fatalf("expected empty stderr, got: %s", stderr.String())
	}
}

func TestRunCheckUsesTaskDirAndDefaultOutput(t *testing.T) {
	tempDir := t.TempDir()
	taskDir := filepath.Join(tempDir, "task")
	if err := os.MkdirAll(taskDir, 0o755); err != nil {
		t.Fatalf("mkdir task dir: %v", err)
	}

	taskJSON := `{
  "task": "bounded task dir change",
  "allowed": ["site/index.html"],
  "blocked": ["docs/research/**"],
  "evidence": ["manual sanity check"]
}`
	if err := os.WriteFile(filepath.Join(taskDir, "input.json"), []byte(taskJSON), 0o644); err != nil {
		t.Fatalf("write task input: %v", err)
	}

	diffBody := `diff --git a/site/index.html b/site/index.html
index 1111111..2222222 100644
--- a/site/index.html
+++ b/site/index.html
@@ -1 +1 @@
-before
+after
`

	var stdout bytes.Buffer
	var stderr bytes.Buffer
	exitCode := run(
		[]string{"check", "--task-dir", taskDir, "--changed-diff", "-"},
		strings.NewReader(diffBody),
		&stdout,
		&stderr,
	)

	if exitCode != 0 {
		t.Fatalf("expected exit code 0, got %d, stderr=%s", exitCode, stderr.String())
	}
	if stdout.Len() != 0 {
		t.Fatalf("expected empty stdout when default output file is used, got: %s", stdout.String())
	}

	outputPath := filepath.Join(taskDir, "generated-review.md")
	body, err := os.ReadFile(outputPath)
	if err != nil {
		t.Fatalf("read generated review: %v", err)
	}
	if !strings.Contains(string(body), "Decision: approve") {
		t.Fatalf("expected approve decision, got:\n%s", string(body))
	}
}
