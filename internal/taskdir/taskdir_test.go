package taskdir

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestInitCreatesTaskDirectoryScaffold(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "task")

	err := Init(dir, InitOptions{
		Task:     "Review a bounded site change.",
		Allowed:  []string{"site/index.html", "./site/style.css"},
		Blocked:  []string{"docs/research/**"},
		Evidence: []string{"manual browser check"},
		Now:      time.Date(2026, 3, 13, 10, 0, 0, 0, time.UTC),
	})
	if err != nil {
		t.Fatalf("init task dir: %v", err)
	}

	files := []string{
		"input.json",
		"intent.md",
		"scope.md",
		"evidence.md",
		"review.md",
	}
	for _, name := range files {
		path := filepath.Join(dir, name)
		if _, err := os.Stat(path); err != nil {
			t.Fatalf("expected %s to exist: %v", path, err)
		}
	}

	inputBody, err := os.ReadFile(filepath.Join(dir, "input.json"))
	if err != nil {
		t.Fatalf("read input.json: %v", err)
	}
	if !strings.Contains(string(inputBody), "\"site/style.css\"") {
		t.Fatalf("expected normalized allowed path, got:\n%s", string(inputBody))
	}

	scopeBody, err := os.ReadFile(filepath.Join(dir, "scope.md"))
	if err != nil {
		t.Fatalf("read scope.md: %v", err)
	}
	if !strings.Contains(string(scopeBody), "- `docs/research/**`") {
		t.Fatalf("expected blocked pattern in scope.md, got:\n%s", string(scopeBody))
	}
}

func TestInitRefusesToOverwriteExistingFiles(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "task")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(dir, "input.json"), []byte("{}\n"), 0o644); err != nil {
		t.Fatalf("write input: %v", err)
	}

	err := Init(dir, InitOptions{
		Task:    "Review a bounded site change.",
		Allowed: []string{"site/index.html"},
	})
	if err == nil {
		t.Fatal("expected overwrite refusal")
	}
	if !strings.Contains(err.Error(), "refusing to overwrite existing file") {
		t.Fatalf("unexpected error: %v", err)
	}
}
