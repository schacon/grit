#!/bin/sh
# Ported subset from git/t/t4013-diff-various.sh for diff-index behavior
# and various diff formatting patterns.

test_description='diff-index default vs -m for missing worktree files, plus various diff patterns'

. ./test-lib.sh

make_commit () {
	msg=$1
	parent=${2-}
	tree=$(git write-tree) || return 1
	if test -n "$parent"
	then
		commit=$(printf '%s\n' "$msg" | git commit-tree "$tree" -p "$parent") || return 1
	else
		commit=$(printf '%s\n' "$msg" | git commit-tree "$tree") || return 1
	fi
	git update-ref HEAD "$commit" || return 1
	printf '%s\n' "$commit"
}

# ===========================================================================
# Part 1: diff-index -m behavior (original tests)
# ===========================================================================

test_expect_success 'setup repository with one tracked file' '
	git init repo &&
	cd repo &&
	printf "one\n" >file1 &&
	git update-index --add file1 &&
	commit1=$(make_commit initial) &&
	test -n "$commit1" &&
	printf "%s\n" "$commit1" >commit1
'

test_expect_success 'diff-index reports removed file by default' '
	cd repo &&
	commit1=$(cat commit1) &&
	rm -f file1 &&
	git diff-index "$commit1" >without_m &&
	lines=$(wc -l <without_m) &&
	test "$lines" -eq 1 &&
	grep " D	file1$" without_m
'

test_expect_success 'diff-index -m hides missing working-tree file' '
	cd repo &&
	commit1=$(cat commit1) &&
	git diff-index -m "$commit1" >with_m &&
	lines=$(wc -l <with_m) &&
	test "$lines" -eq 0
'

test_expect_success '--cached mode ignores missing working-tree file' '
	cd repo &&
	commit1=$(cat commit1) &&
	git diff-index --cached --exit-code "$commit1"
'

# ===========================================================================
# Part 2: Multi-file diff-index tests
# ===========================================================================

test_expect_success 'setup multi-file repository' '
	git init repo2 &&
	cd repo2 &&
	printf "1\n2\n3\n" >file0 &&
	printf "A\nB\n" >dir_sub &&
	git update-index --add file0 dir_sub &&
	commit1=$(make_commit "Initial") &&
	printf "%s\n" "$commit1" >commit1 &&
	printf "1\n2\n3\n4\n5\n6\n" >file0 &&
	git update-index file0 &&
	commit2=$(make_commit "Second" "$commit1") &&
	printf "%s\n" "$commit2" >commit2
'

test_expect_success 'diff-index worktree mode detects unstaged changes' '
	cd repo2 &&
	c2=$(cat commit2) &&
	printf "modified\n" >file0 &&
	git diff-index "$c2" >out &&
	grep "M" out &&
	grep "file0" out
'

test_expect_success 'diff-index --quiet returns 1 for differences' '
	cd repo2 &&
	c1=$(cat commit1) &&
	test_must_fail git diff-index --quiet --cached "$c1"
'

test_expect_success 'diff-index --quiet returns 0 when identical' '
	cd repo2 &&
	c2=$(cat commit2) &&
	git diff-index --quiet --cached "$c2"
'

test_expect_success 'diff-index raw output shows correct fields' '
	cd repo2 &&
	c1=$(cat commit1) &&
	git diff-index --cached "$c1" >out &&
	grep "^:" out &&
	grep "file0" out
'

test_expect_success 'diff-index --exit-code returns 0 when same' '
	cd repo2 &&
	c2=$(cat commit2) &&
	printf "1\n2\n3\n4\n5\n6\n" >file0 &&
	git diff-index --exit-code "$c2"
'

test_expect_success 'diff-index --exit-code returns 1 when different' '
	cd repo2 &&
	c1=$(cat commit1) &&
	test_must_fail git diff-index --exit-code --cached "$c1"
'

test_expect_success 'diff-index with multiple changed files' '
	cd repo2 &&
	c1=$(cat commit1) &&
	git diff-index --cached "$c1" >out &&
	lines=$(wc -l <out) &&
	test "$lines" -ge 1
'

test_expect_success 'diff-index detects deletion of worktree file' '
	cd repo2 &&
	c2=$(cat commit2) &&
	printf "1\n2\n3\n4\n5\n6\n" >file0 &&
	git update-index file0 &&
	rm -f dir_sub &&
	git diff-index "$c2" >out &&
	grep "D" out &&
	grep "dir_sub" out
'

# ===========================================================================
# Part 3: diff-index -m with multiple files (from t4013 patterns)
# ===========================================================================

test_expect_success 'setup repo with multiple tracked files for -m tests' '
	git init repo_m &&
	cd repo_m &&
	printf "one\n" >file1 &&
	printf "two\n" >file2 &&
	printf "three\n" >file3 &&
	git update-index --add file1 file2 file3 &&
	commit1=$(make_commit "initial 3 files") &&
	printf "%s\n" "$commit1" >commit1
'

test_expect_success 'diff-index -m: remove one of several files, only shows remaining diffs' '
	cd repo_m &&
	c1=$(cat commit1) &&
	printf "modified\n" >file2 &&
	rm -f file3 &&
	git diff-index "$c1" >without_m &&
	git diff-index -m "$c1" >with_m &&
	# without -m should show both file2 (modified worktree) and file3 (deleted)
	grep "file2" without_m &&
	grep "file3" without_m &&
	# with -m should hide file3 (missing worktree) but still show file2
	grep "file2" with_m &&
	! grep "file3" with_m
'

test_expect_success 'diff-index -m: remove all files, output is empty' '
	cd repo_m &&
	c1=$(cat commit1) &&
	rm -f file1 file2 file3 &&
	git diff-index -m "$c1" >with_m &&
	test_must_be_empty with_m
'

test_expect_success 'diff-index: all files missing shows D for each' '
	cd repo_m &&
	c1=$(cat commit1) &&
	rm -f file1 file2 file3 &&
	git diff-index "$c1" >out &&
	test_line_count = 3 out &&
	grep "file1" out &&
	grep "file2" out &&
	grep "file3" out
'

# ===========================================================================
# Part 4: diff-index raw output field validation
# ===========================================================================

test_expect_success 'setup clean repo for raw field validation' '
	git init repo_raw &&
	cd repo_raw &&
	printf "content\n" >tracked.txt &&
	git update-index --add tracked.txt &&
	commit1=$(make_commit "initial") &&
	printf "%s\n" "$commit1" >commit1 &&
	printf "modified content\n" >tracked.txt &&
	git update-index tracked.txt &&
	commit2=$(make_commit "modify" "$commit1") &&
	printf "%s\n" "$commit2" >commit2
'

test_expect_success 'diff-index raw output has colon-prefixed lines' '
	cd repo_raw &&
	c1=$(cat commit1) &&
	git diff-index --cached "$c1" >out &&
	grep "^:" out
'

test_expect_success 'diff-index raw output has 6-digit mode fields' '
	cd repo_raw &&
	c1=$(cat commit1) &&
	git diff-index --cached "$c1" >out &&
	grep "^:[0-9]\{6\} [0-9]\{6\} " out
'

test_expect_success 'diff-index raw output OIDs are 40 hex chars' '
	cd repo_raw &&
	c1=$(cat commit1) &&
	git diff-index --cached "$c1" >out &&
	awk "{print \$3; print \$4}" out >oids &&
	grep -E "^[0-9a-f]{40}$" oids
'

test_expect_success 'diff-index raw output status letter matches expected' '
	cd repo_raw &&
	c1=$(cat commit1) &&
	git diff-index --cached "$c1" >out &&
	grep "M	tracked.txt" out
'

# ===========================================================================
# Part 5: diff-tree with various formatting (from t4013 patterns)
# ===========================================================================

test_expect_success 'setup repo for diff-tree formatting tests' '
	git init repo_dt &&
	cd repo_dt &&
	printf "1\n2\n3\n" >file0 &&
	printf "A\nB\n" >dir_sub &&
	git update-index --add file0 dir_sub &&
	commit1=$(make_commit "Initial") &&
	printf "%s\n" "$commit1" >commit1 &&
	printf "1\n2\n3\n4\n5\n6\n" >file0 &&
	printf "A\nB\nC\nD\n" >dir_sub &&
	git update-index file0 dir_sub &&
	commit2=$(make_commit "Second" "$commit1") &&
	printf "%s\n" "$commit2" >commit2 &&
	printf "A\nB\nC\n" >file1 &&
	git update-index --add file1 &&
	commit3=$(make_commit "Third" "$commit2") &&
	printf "%s\n" "$commit3" >commit3
'

test_expect_success 'diff-tree single commit shows raw diff' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	git diff-tree "$c2" >out &&
	grep "^:" out
'

test_expect_success 'diff-tree -r single commit lists all changed files' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	git diff-tree -r "$c2" >out &&
	grep "file0" out &&
	grep "dir_sub" out
'

test_expect_success 'diff-tree --name-only shows only filenames' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	git diff-tree -r --name-only "$c2" >out &&
	grep "^file0$" out &&
	grep "^dir_sub$" out &&
	! grep "^:" out
'

test_expect_success 'diff-tree --name-status shows letter and filename' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	git diff-tree -r --name-status "$c2" >out &&
	grep "^M	file0" out &&
	grep "^M	dir_sub" out
'

test_expect_success 'diff-tree --stat shows diffstat summary' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	git diff-tree -r --stat "$c2" >out &&
	grep "file0" out &&
	grep "dir_sub" out &&
	grep "changed" out
'

test_expect_success 'diff-tree -p produces unified diff' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	git diff-tree -r -p "$c2" >out &&
	grep "^diff --git" out &&
	grep "^---" out &&
	grep "^+++" out &&
	grep "^@@" out
'

test_expect_success 'diff-tree root commit without --root is empty' '
	cd repo_dt &&
	c1=$(cat commit1) &&
	git diff-tree "$c1" >out &&
	test_must_be_empty out
'

test_expect_success 'diff-tree root commit with --root shows adds' '
	cd repo_dt &&
	c1=$(cat commit1) &&
	git diff-tree -r --root "$c1" >out &&
	grep "A	file0" out &&
	grep "A	dir_sub" out
'

test_expect_success 'diff-tree --root -p on root commit shows patches' '
	cd repo_dt &&
	c1=$(cat commit1) &&
	git diff-tree -r --root -p "$c1" >out &&
	grep "^new file mode 100644" out &&
	grep "^diff --git a/file0 b/file0" out
'

test_expect_success 'diff-tree two commits directly' '
	cd repo_dt &&
	c1=$(cat commit1) &&
	c3=$(cat commit3) &&
	git diff-tree -r "$c1" "$c3" >out &&
	grep "file0" out &&
	grep "dir_sub" out &&
	grep "file1" out
'

test_expect_success 'diff-tree two same commits produces no output' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	git diff-tree "$c2" "$c2" >out &&
	test_must_be_empty out
'

# ===========================================================================
# Part 6: diff-tree --stdin patterns (from t4013)
# ===========================================================================

test_expect_success 'diff-tree --stdin with log formatting: commit header' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	printf "%s\n" "$c2" | git diff-tree --stdin >out &&
	head -1 out | grep "^[0-9a-f]\{40\}$"
'

test_expect_success 'diff-tree --stdin --no-commit-id suppresses header' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	printf "%s\n" "$c2" | git diff-tree --stdin --no-commit-id >out &&
	! head -1 out | grep "^[0-9a-f]\{40\}$" &&
	grep "^:" out
'

test_expect_success 'diff-tree --stdin -v shows commit message' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	printf "%s\n" "$c2" | git diff-tree --stdin -v >out &&
	grep "Second" out
'

test_expect_success 'diff-tree --stdin -s suppresses diff output' '
	cd repo_dt &&
	c2=$(cat commit2) &&
	printf "%s\n" "$c2" | git diff-tree --stdin -s >out &&
	head -1 out | grep "^[0-9a-f]\{40\}$" &&
	! grep "^:" out
'

test_expect_success 'diff-tree --stdin with two tree OIDs' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	t1=$(git cat-file -p "$c1" | grep "^tree" | awk "{print \$2}") &&
	t2=$(git cat-file -p "$c2" | grep "^tree" | awk "{print \$2}") &&
	printf "%s %s\n" "$t1" "$t2" | git diff-tree --stdin >out &&
	grep "file0" out
'

test_expect_success 'diff-tree --stdin processes multiple commits' '
	cd repo_dt &&
	c2=$(cat commit2) && c3=$(cat commit3) &&
	printf "%s\n%s\n" "$c2" "$c3" | git diff-tree --stdin >out &&
	grep "file0" out &&
	grep "file1" out
'

test_expect_success 'diff-tree --stdin with pathspec limits output' '
	cd repo_dt &&
	c3=$(cat commit3) &&
	printf "%s\n" "$c3" | git diff-tree -r --stdin --name-only "$c3" -- file1 >out &&
	grep "file1" out
'

# ===========================================================================
# Part 7: diff --cached patterns (from t4013)
# ===========================================================================

test_expect_success 'setup repo for diff --cached tests' '
	git init repo_cached &&
	cd repo_cached &&
	printf "initial\n" >file.txt &&
	git update-index --add file.txt &&
	commit1=$(make_commit "first") &&
	printf "%s\n" "$commit1" >commit1
'

test_expect_success 'diff --cached with no changes is empty' '
	cd repo_cached &&
	git diff --cached >out &&
	test_must_be_empty out
'

test_expect_success 'diff --cached shows staged additions' '
	cd repo_cached &&
	printf "new content\n" >newfile.txt &&
	git update-index --add newfile.txt &&
	git diff --cached --name-only >out &&
	grep "newfile.txt" out
'

test_expect_success 'diff --cached --stat shows diffstat' '
	cd repo_cached &&
	git diff --cached --stat >out &&
	grep "newfile.txt" out &&
	grep "changed" out
'

test_expect_success 'diff --cached --name-status shows A for new file' '
	cd repo_cached &&
	git diff --cached --name-status >out &&
	grep "^A	newfile.txt" out
'

test_expect_success 'diff --cached --exit-code returns 1 for staged changes' '
	cd repo_cached &&
	test_must_fail git diff --cached --exit-code
'

test_expect_success 'diff --cached --quiet returns 1 for staged changes' '
	cd repo_cached &&
	test_must_fail git diff --cached --quiet
'

test_expect_success 'diff --cached --exit-code returns 0 when index matches HEAD' '
	cd repo_cached &&
	c1=$(cat commit1) &&
	git read-tree "$c1" &&
	git diff --cached --exit-code
'

# ===========================================================================
# Part 8: diff --stat/--numstat/--name-only/--name-status between commits
# ===========================================================================

test_expect_success 'diff --stat between commits' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff --stat "$c1" "$c2" >actual &&
	test_line_count -gt 0 actual
'

test_expect_success 'diff --numstat between commits' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff --numstat "$c1" "$c2" >actual &&
	test_line_count -gt 0 actual
'

test_expect_success 'diff --name-only between commits' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff --name-only "$c1" "$c2" >actual &&
	test_line_count -gt 0 actual
'

test_expect_success 'diff --name-status between commits' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff --name-status "$c1" "$c2" >actual &&
	test_line_count -gt 0 actual
'

test_expect_success 'diff --stat between commits shows file names' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff --stat "$c1" "$c2" >actual &&
	grep "file0" actual &&
	grep "dir_sub" actual
'

test_expect_success 'diff --numstat between commits shows numeric counts' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff --numstat "$c1" "$c2" >actual &&
	grep "file0" actual &&
	grep "^[0-9]" actual
'

test_expect_success 'diff --name-only between commits lists file names only' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff --name-only "$c1" "$c2" >actual &&
	grep "^file0$" actual &&
	grep "^dir_sub$" actual
'

test_expect_success 'diff --name-status between commits shows M status' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff --name-status "$c1" "$c2" >actual &&
	grep "^M" actual
'

test_expect_success 'diff --stat summary line shows changed count' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff --stat "$c1" "$c2" >actual &&
	grep "changed" actual
'

test_expect_success 'diff --exit-code between different commits returns 1' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	test_must_fail git diff --exit-code "$c1" "$c2"
'

# ===========================================================================
# Part 9: diff unified output validation
# ===========================================================================

test_expect_success 'diff unified output between commits has diff header' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff "$c1" "$c2" >actual &&
	grep "^diff --git" actual
'

test_expect_success 'diff unified output between commits has --- and +++' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff "$c1" "$c2" >actual &&
	grep "^---" actual &&
	grep "^+++" actual
'

test_expect_success 'diff unified output between commits has hunk header' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff "$c1" "$c2" >actual &&
	grep "^@@" actual
'

test_expect_success 'diff --quiet between different commits returns 1' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	test_must_fail git diff --quiet "$c1" "$c2"
'

test_expect_success 'diff --quiet between same commits returns 0' '
	cd repo_dt &&
	c1=$(cat commit1) &&
	git diff --quiet "$c1" "$c1"
'

# ===========================================================================
# Part 10: diff -U context control between commits
# ===========================================================================

test_expect_success 'diff -U0 between commits shows no context lines' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff -U0 "$c1" "$c2" >actual &&
	grep "^@@" actual &&
	grep "^+" actual
'

test_expect_success 'diff -U1 between commits reduces context' '
	cd repo_dt &&
	c1=$(cat commit1) && c2=$(cat commit2) &&
	git diff -U1 "$c1" "$c2" >actual &&
	grep "^@@" actual
'

# ===========================================================================
# Part 11: diff with three commits (c1->c2->c3)
# ===========================================================================

test_expect_success 'diff --name-only across two commits shows all changed files' '
	cd repo_dt &&
	c1=$(cat commit1) && c3=$(cat commit3) &&
	git diff --name-only "$c1" "$c3" >actual &&
	grep "file0" actual &&
	grep "dir_sub" actual &&
	grep "file1" actual
'

test_expect_success 'diff --name-status across two commits shows status' '
	cd repo_dt &&
	c1=$(cat commit1) && c3=$(cat commit3) &&
	git diff --name-status "$c1" "$c3" >actual &&
	test_line_count -ge 3 actual
'

test_expect_success 'diff --stat across two commits shows summary' '
	cd repo_dt &&
	c1=$(cat commit1) && c3=$(cat commit3) &&
	git diff --stat "$c1" "$c3" >actual &&
	grep "file0" actual &&
	grep "file1" actual
'

# ===========================================================================
# Part 12: diff --cached with modifications
# ===========================================================================

test_expect_success 'setup repo for diff --cached modification tests' '
	git init repo_cached2 &&
	cd repo_cached2 &&
	printf "original\n" >file.txt &&
	git update-index --add file.txt &&
	commit1=$(make_commit "first") &&
	printf "%s\n" "$commit1" >commit1 &&
	printf "modified\n" >file.txt &&
	git update-index file.txt
'

test_expect_success 'diff --cached --name-only shows modified file' '
	cd repo_cached2 &&
	git diff --cached --name-only >out &&
	grep "file.txt" out
'

test_expect_success 'diff --cached --name-status shows M for modified file' '
	cd repo_cached2 &&
	git diff --cached --name-status >out &&
	grep "^M" out &&
	grep "file.txt" out
'

test_expect_success 'diff --cached --stat shows modified file in summary' '
	cd repo_cached2 &&
	git diff --cached --stat >out &&
	grep "file.txt" out &&
	grep "changed" out
'

test_expect_success 'diff --cached --numstat shows counts for modified file' '
	cd repo_cached2 &&
	git diff --cached --numstat >out &&
	grep "file.txt" out &&
	grep "^[0-9]" out
'

# ===========================================================================
# Part 13: diff worktree (unstaged) tests
# ===========================================================================

test_expect_success 'setup repo for unstaged diff tests' '
	git init repo_unstaged &&
	cd repo_unstaged &&
	printf "line1\nline2\nline3\n" >data.txt &&
	git update-index --add data.txt &&
	c1=$(make_commit "base") &&
	printf "%s\n" "$c1" >commit1 &&
	printf "line1\nMODIFIED\nline3\n" >data.txt
'

test_expect_success 'diff (unstaged) shows changes' '
	cd repo_unstaged &&
	git diff >out &&
	grep "^diff --git" out
'

test_expect_success 'diff --name-only (unstaged) shows modified file' '
	cd repo_unstaged &&
	git diff --name-only >out &&
	grep "^data.txt$" out
'

test_expect_success 'diff --name-status (unstaged) shows M' '
	cd repo_unstaged &&
	git diff --name-status >out &&
	grep "^M" out
'

test_expect_success 'diff --stat (unstaged) shows diffstat' '
	cd repo_unstaged &&
	git diff --stat >out &&
	grep "data.txt" out &&
	grep "changed" out
'

test_expect_success 'diff --numstat (unstaged) shows numeric counts' '
	cd repo_unstaged &&
	git diff --numstat >out &&
	grep "data.txt" out &&
	grep "^[0-9]" out
'

test_expect_success 'diff --exit-code (unstaged) returns 1 for changes' '
	cd repo_unstaged &&
	test_must_fail git diff --exit-code
'

test_expect_success 'diff --quiet (unstaged) returns 1 for changes' '
	cd repo_unstaged &&
	test_must_fail git diff --quiet
'

test_expect_success 'diff --quiet (unstaged) suppresses output' '
	cd repo_unstaged &&
	git diff --quiet >out 2>&1 || true &&
	test_must_be_empty out
'

test_expect_success 'diff -U0 (unstaged) shows zero context' '
	cd repo_unstaged &&
	git diff -U0 >out &&
	grep "^@@" out
'

test_expect_success 'diff (unstaged) unified output shows removed line with -' '
	cd repo_unstaged &&
	git diff >out &&
	grep "^-line2" out
'

# ===========================================================================
# Part 14: diff with pathspec filtering (commit-to-commit)
# ===========================================================================

test_expect_success 'setup pathspec repo for diff' '
	git init repo_pathspec &&
	cd repo_pathspec &&
	mkdir sub &&
	printf "root content\n" >root.txt &&
	printf "nested content\n" >sub/nested.txt &&
	git update-index --add root.txt sub/nested.txt &&
	c1=$(make_commit "initial") &&
	printf "%s\n" "$c1" >../ps_c1 &&
	printf "root modified\n" >root.txt &&
	printf "nested modified\n" >sub/nested.txt &&
	git update-index root.txt sub/nested.txt &&
	c2=$(make_commit "modify both" "$c1") &&
	printf "%s\n" "$c2" >../ps_c2
'

test_expect_success 'diff with pathspec -- sub shows only sub/' '
	cd repo_pathspec &&
	c1=$(cat ../ps_c1) && c2=$(cat ../ps_c2) &&
	git diff --name-only "$c1" "$c2" -- sub >out &&
	grep "sub/nested.txt" out &&
	! grep "root.txt" out
'

test_expect_success 'diff with pathspec -- root.txt shows only root.txt' '
	cd repo_pathspec &&
	c1=$(cat ../ps_c1) && c2=$(cat ../ps_c2) &&
	git diff --name-only "$c1" "$c2" -- root.txt >out &&
	grep "root.txt" out &&
	! grep "nested" out
'

test_expect_success 'diff with non-matching pathspec is empty' '
	cd repo_pathspec &&
	c1=$(cat ../ps_c1) && c2=$(cat ../ps_c2) &&
	git diff --name-only "$c1" "$c2" -- nonexistent >out &&
	test_must_be_empty out
'

test_expect_success 'diff --exit-code with non-matching pathspec returns 0' '
	cd repo_pathspec &&
	c1=$(cat ../ps_c1) && c2=$(cat ../ps_c2) &&
	git diff --exit-code "$c1" "$c2" -- nonexistent
'

test_expect_success 'diff --stat with pathspec shows only matching file' '
	cd repo_pathspec &&
	c1=$(cat ../ps_c1) && c2=$(cat ../ps_c2) &&
	git diff --stat "$c1" "$c2" -- root.txt >out &&
	grep "root.txt" out &&
	! grep "nested" out
'

# ===========================================================================
# Part 15: diff with binary files
# ===========================================================================

test_expect_success 'setup repo with binary file' '
	git init repo_binary &&
	cd repo_binary &&
	printf "text content\n" >text.txt &&
	git update-index --add text.txt &&
	c1=$(make_commit "initial text") &&
	printf "%s\n" "$c1" >../bin_c1 &&
	printf "\000\001\002\003" >binary.dat &&
	git update-index --add binary.dat &&
	c2=$(make_commit "add binary" "$c1") &&
	printf "%s\n" "$c2" >../bin_c2
'

test_expect_success 'diff --name-only shows binary file' '
	cd repo_binary &&
	c1=$(cat ../bin_c1) && c2=$(cat ../bin_c2) &&
	git diff --name-only "$c1" "$c2" >out &&
	grep "binary.dat" out
'

test_expect_success 'diff --name-status shows A for new binary file' '
	cd repo_binary &&
	c1=$(cat ../bin_c1) && c2=$(cat ../bin_c2) &&
	git diff --name-status "$c1" "$c2" >out &&
	grep "A" out &&
	grep "binary.dat" out
'

test_expect_success 'diff --stat shows binary file in stats' '
	cd repo_binary &&
	c1=$(cat ../bin_c1) && c2=$(cat ../bin_c2) &&
	git diff --stat "$c1" "$c2" >out &&
	grep "binary.dat" out
'

test_expect_success 'diff shows new file mode for binary' '
	cd repo_binary &&
	c1=$(cat ../bin_c1) && c2=$(cat ../bin_c2) &&
	git diff "$c1" "$c2" >out &&
	grep "new file mode" out
'

# ===========================================================================
# Part 16: diff with executable file permissions
# ===========================================================================

test_expect_success 'setup repo with executable file' '
	git init repo_exec &&
	cd repo_exec &&
	printf "normal\n" >script.sh &&
	git update-index --add script.sh &&
	c1=$(make_commit "normal file") &&
	printf "%s\n" "$c1" >../exec_c1 &&
	chmod +x script.sh &&
	printf "executable\n" >run.sh &&
	chmod +x run.sh &&
	git update-index --add run.sh &&
	c2=$(make_commit "add executable" "$c1") &&
	printf "%s\n" "$c2" >../exec_c2
'

test_expect_success 'diff shows executable mode 100755 for new file' '
	cd repo_exec &&
	c1=$(cat ../exec_c1) && c2=$(cat ../exec_c2) &&
	git diff "$c1" "$c2" >out &&
	grep "new file mode 100755" out
'

test_expect_success 'diff-tree shows 100755 mode for executable' '
	cd repo_exec &&
	c1=$(cat ../exec_c1) && c2=$(cat ../exec_c2) &&
	git diff-tree -r "$c1" "$c2" >out &&
	grep "100755" out &&
	grep "run.sh" out
'

test_expect_success 'diff --name-status shows A for executable file' '
	cd repo_exec &&
	c1=$(cat ../exec_c1) && c2=$(cat ../exec_c2) &&
	git diff --name-status "$c1" "$c2" >out &&
	grep "A" out &&
	grep "run.sh" out
'

# ===========================================================================
# Part 17: diff between arbitrary commits (not parent-child)
# ===========================================================================

test_expect_success 'setup repo with branching history' '
	git init repo_arb &&
	cd repo_arb &&
	printf "base\n" >file.txt &&
	git update-index --add file.txt &&
	c1=$(make_commit "base") &&
	printf "%s\n" "$c1" >../arb_c1 &&
	printf "version A\n" >file.txt &&
	git update-index file.txt &&
	c2=$(make_commit "branch A" "$c1") &&
	printf "%s\n" "$c2" >../arb_c2 &&
	printf "version B\n" >file.txt &&
	git update-index file.txt &&
	c3=$(make_commit "branch B" "$c1") &&
	printf "%s\n" "$c3" >../arb_c3
'

test_expect_success 'diff between non-parent-child commits works' '
	cd repo_arb &&
	c2=$(cat ../arb_c2) && c3=$(cat ../arb_c3) &&
	git diff "$c2" "$c3" >out &&
	grep "^diff --git" out &&
	grep "^-version A" out &&
	grep "^+version B" out
'

test_expect_success 'diff --name-only between arbitrary commits' '
	cd repo_arb &&
	c2=$(cat ../arb_c2) && c3=$(cat ../arb_c3) &&
	git diff --name-only "$c2" "$c3" >out &&
	grep "file.txt" out
'

test_expect_success 'diff --stat between arbitrary commits' '
	cd repo_arb &&
	c2=$(cat ../arb_c2) && c3=$(cat ../arb_c3) &&
	git diff --stat "$c2" "$c3" >out &&
	grep "file.txt" out &&
	grep "changed" out
'

test_expect_success 'diff --numstat between arbitrary commits' '
	cd repo_arb &&
	c2=$(cat ../arb_c2) && c3=$(cat ../arb_c3) &&
	git diff --numstat "$c2" "$c3" >out &&
	grep "file.txt" out
'

test_expect_success 'diff --exit-code between arbitrary commits returns 1' '
	cd repo_arb &&
	c2=$(cat ../arb_c2) && c3=$(cat ../arb_c3) &&
	test_must_fail git diff --exit-code "$c2" "$c3"
'

test_expect_success 'diff --quiet between arbitrary commits returns 1' '
	cd repo_arb &&
	c2=$(cat ../arb_c2) && c3=$(cat ../arb_c3) &&
	test_must_fail git diff --quiet "$c2" "$c3"
'

# ===========================================================================
# Part 18: diff with deleted files between commits
# ===========================================================================

test_expect_success 'setup repo with file deletion' '
	git init repo_del &&
	cd repo_del &&
	printf "to be deleted\n" >doomed.txt &&
	printf "survivor\n" >kept.txt &&
	git update-index --add doomed.txt kept.txt &&
	c1=$(make_commit "two files") &&
	printf "%s\n" "$c1" >../del_c1 &&
	git update-index --remove doomed.txt &&
	rm -f doomed.txt &&
	c2=$(make_commit "delete one" "$c1") &&
	printf "%s\n" "$c2" >../del_c2
'

test_expect_success 'diff shows deleted file mode header' '
	cd repo_del &&
	c1=$(cat ../del_c1) && c2=$(cat ../del_c2) &&
	git diff "$c1" "$c2" >out &&
	grep "^deleted file mode" out
'

test_expect_success 'diff --name-status shows D for deleted file' '
	cd repo_del &&
	c1=$(cat ../del_c1) && c2=$(cat ../del_c2) &&
	git diff --name-status "$c1" "$c2" >out &&
	grep "D" out &&
	grep "doomed.txt" out
'

test_expect_success 'diff pathspec on deleted file shows it' '
	cd repo_del &&
	c1=$(cat ../del_c1) && c2=$(cat ../del_c2) &&
	git diff --name-only "$c1" "$c2" -- doomed.txt >out &&
	grep "doomed.txt" out
'

test_expect_success 'diff pathspec on kept file shows nothing' '
	cd repo_del &&
	c1=$(cat ../del_c1) && c2=$(cat ../del_c2) &&
	git diff --name-only "$c1" "$c2" -- kept.txt >out &&
	test_must_be_empty out
'

test_done
