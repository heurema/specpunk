package main

import (
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"specpunk/internal/check"
	"specpunk/internal/taskdir"
)

func main() {
	os.Exit(run(os.Args[1:], os.Stdin, os.Stdout, os.Stderr))
}

func run(args []string, stdin io.Reader, stdout io.Writer, stderr io.Writer) int {
	if len(args) == 0 {
		printUsage(stderr)
		return 2
	}

	switch args[0] {
	case "check":
		return runCheck(args[1:], stdin, stdout, stderr)
	case "task":
		return runTask(args[1:], stdout, stderr)
	default:
		fmt.Fprintf(stderr, "error: unknown command %q\n\n", args[0])
		printUsage(stderr)
		return 2
	}
}

func runCheck(args []string, stdin io.Reader, stdout io.Writer, stderr io.Writer) int {
	flags := flag.NewFlagSet("check", flag.ContinueOnError)
	flags.SetOutput(stderr)

	taskPath := flags.String("task", "", "Path to the structured task JSON file.")
	taskDir := flags.String("task-dir", "", "Path to a task directory containing input.json.")
	changedManifestPath := flags.String("changed-manifest", "", "Path to a changed-file manifest. Supports newline-delimited text or a JSON array.")
	changedDiffPath := flags.String("changed-diff", "", "Path to a unified diff or git patch file. Use '-' to read from stdin.")
	changedGitRange := flags.String("changed-git", "", "Git revspec passed to 'git diff --name-only --relative'. Example: HEAD~1..HEAD")
	outputPath := flags.String("output", "", "Path to write the markdown artifact.")

	if err := flags.Parse(args); err != nil {
		return 2
	}

	if (*taskPath == "" && *taskDir == "") || (*taskPath != "" && *taskDir != "") {
		fmt.Fprintln(stderr, "error: use exactly one task source: --task or --task-dir")
		return 2
	}

	resolvedTaskPath := *taskPath
	if *taskDir != "" {
		resolvedTaskPath = filepath.Join(*taskDir, "input.json")
	}

	sourceCount := 0
	if *changedManifestPath != "" {
		sourceCount++
	}
	if *changedDiffPath != "" {
		sourceCount++
	}
	if *changedGitRange != "" {
		sourceCount++
	}
	if sourceCount > 1 {
		fmt.Fprintln(stderr, "error: use only one changed-file source: --changed-manifest, --changed-diff, or --changed-git")
		return 2
	}

	workingDir, err := os.Getwd()
	if err != nil {
		fmt.Fprintf(stderr, "error: %v\n", err)
		return 2
	}

	content, err := check.GenerateFromFile(resolvedTaskPath, check.GenerateOptions{
		ChangedManifestPath: *changedManifestPath,
		ChangedDiffPath:     *changedDiffPath,
		ChangedDiffReader:   stdin,
		ChangedGitRange:     *changedGitRange,
		WorkingDir:          workingDir,
	})
	if err != nil {
		fmt.Fprintf(stderr, "error: %v\n", err)
		return 2
	}

	resolvedOutputPath := *outputPath
	if resolvedOutputPath == "" && *taskDir != "" {
		resolvedOutputPath = filepath.Join(*taskDir, "generated-review.md")
	}

	if resolvedOutputPath == "" {
		_, _ = io.WriteString(stdout, content)
		return 0
	}

	if err := os.MkdirAll(filepath.Dir(resolvedOutputPath), 0o755); err != nil {
		fmt.Fprintf(stderr, "error: %v\n", err)
		return 2
	}
	if err := os.WriteFile(resolvedOutputPath, []byte(content), 0o644); err != nil {
		fmt.Fprintf(stderr, "error: %v\n", err)
		return 2
	}

	return 0
}

func runTask(args []string, stdout io.Writer, stderr io.Writer) int {
	if len(args) == 0 {
		printUsage(stderr)
		return 2
	}

	switch args[0] {
	case "init":
		return runTaskInit(args[1:], stdout, stderr)
	default:
		fmt.Fprintf(stderr, "error: unknown task command %q\n\n", args[0])
		printUsage(stderr)
		return 2
	}
}

func runTaskInit(args []string, stdout io.Writer, stderr io.Writer) int {
	flags := flag.NewFlagSet("task init", flag.ContinueOnError)
	flags.SetOutput(stderr)

	taskDirPath := flags.String("task-dir", "", "Path to the task directory to create.")
	taskText := flags.String("task", "", "Task description stored in input.json.")
	var allowed multiFlag
	var blocked multiFlag
	var evidence multiFlag
	flags.Var(&allowed, "allow", "Allowed path or pattern. Repeat for multiple values.")
	flags.Var(&blocked, "block", "Blocked path or pattern. Repeat for multiple values.")
	flags.Var(&evidence, "evidence", "Expected evidence note. Repeat for multiple values.")

	if err := flags.Parse(args); err != nil {
		return 2
	}

	if err := taskdir.Init(*taskDirPath, taskdir.InitOptions{
		Task:     *taskText,
		Allowed:  allowed,
		Blocked:  blocked,
		Evidence: evidence,
	}); err != nil {
		fmt.Fprintf(stderr, "error: %v\n", err)
		return 2
	}

	_, _ = fmt.Fprintf(stdout, "created task scaffold in %s\n", *taskDirPath)
	return 0
}

type multiFlag []string

func (f *multiFlag) String() string {
	return fmt.Sprintf("%v", []string(*f))
}

func (f *multiFlag) Set(value string) error {
	*f = append(*f, value)
	return nil
}

func printUsage(w io.Writer) {
	fmt.Fprintln(w, "Usage:")
	fmt.Fprintln(w, "  specpunk check (--task <path> | --task-dir <dir>) [--changed-manifest <path> | --changed-diff <path|-> | --changed-git <revspec>] [--output <path>]")
	fmt.Fprintln(w, "  specpunk task init --task-dir <dir> --task <text> --allow <pattern> [--allow <pattern> ...] [--block <pattern> ...] [--evidence <text> ...]")
}
