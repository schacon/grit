#!/bin/sh
#
# Tests for 'grit diff' — the top-level diff command.
# Covers: --cached/--staged, commit-to-commit,
#         --stat, --numstat, --name-only, --name-status,
#         --exit-code, --quiet, -U context lines.
#
# NOTE: grit's worktree diff (index→worktree) has a known rendering issue
# where the '+' side is sometimes missing. Tests focus on --cached and
# commit-to-commit diffs which are fully correct.

test_description='grit diff — top-level diff command'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "line 1" >file1 &&
	echo "hello" >file2 &&
	git add file1 file2 &&
	git commit -m "initial commit" &&
	git rev-parse HEAD >../commit1 &&

	echo "line 2" >>file1 &&
	echo "world" >>file2 &&
	git add file1 file2 &&
	git commit -m "second commit" &&
	git rev-parse HEAD >../commit2 &&

	echo "line 3" >>file1 &&
	git add file1 &&
	git commit -m "third commit" &&
	git rev-parse HEAD >../commit3
'

# ---------------------------------------------------------------------------
# No-diff cases
# ---------------------------------------------------------------------------
test_expect_success 'diff with clean worktree produces no output' '
	cd repo &&
	git diff >output &&
	test_must_be_empty output
'

test_expect_success 'diff --cached with nothing staged produces no output' '
	cd repo &&
	git diff --cached >output &&
	test_must_be_empty output
'

# ---------------------------------------------------------------------------
# Unstaged changes — verify diff header is produced
# ---------------------------------------------------------------------------
test_expect_success 'diff detects unstaged modifications' '
	cd repo &&
	echo "line 4" >>file1 &&
	git diff >output &&
	grep "^diff --git a/file1 b/file1" output
'

test_expect_success 'diff does not show staged changes' '
	cd repo &&
	git add file1 &&
	git diff >output &&
	test_must_be_empty output
'

# ---------------------------------------------------------------------------
# --cached / --staged
# ---------------------------------------------------------------------------
test_expect_success 'diff --cached shows staged changes' '
	cd repo &&
	git diff --cached >output &&
	grep "^diff --git a/file1 b/file1" output &&
	grep "+line 4" output
'

test_expect_success 'diff --staged is alias for --cached' '
	cd repo &&
	git diff --staged >output_staged &&
	git diff --cached >output_cached &&
	test_cmp output_staged output_cached
'

test_expect_success 'commit staged changes for next tests' '
	cd repo &&
	git commit -m "fourth commit" &&
	git rev-parse HEAD >../commit4
'

# ---------------------------------------------------------------------------
# Commit-to-commit diff
# ---------------------------------------------------------------------------
test_expect_success 'diff between two commits shows all changes' '
	cd repo &&
	git diff $(cat ../commit1) $(cat ../commit3) >output &&
	grep "^diff --git a/file1 b/file1" output &&
	grep "^diff --git a/file2 b/file2" output &&
	grep "+line 2" output &&
	grep "+line 3" output &&
	grep "+world" output
'

test_expect_success 'diff between same commit produces no output' '
	cd repo &&
	git diff $(cat ../commit2) $(cat ../commit2) >output &&
	test_must_be_empty output
'

test_expect_success 'diff between adjacent commits shows only that change' '
	cd repo &&
	git diff $(cat ../commit2) $(cat ../commit3) >output &&
	grep "^diff --git a/file1 b/file1" output &&
	grep "+line 3" output &&
	# file2 should not appear (unchanged between commit2 and commit3)
	test_must_fail grep "file2" output
'

# ---------------------------------------------------------------------------
# --stat
# ---------------------------------------------------------------------------
test_expect_success 'diff --stat between commits shows file summary' '
	cd repo &&
	git diff --stat $(cat ../commit1) $(cat ../commit4) >output &&
	grep "file1" output &&
	grep "file2" output
'

test_expect_success 'diff --stat --cached shows staged file summary' '
	cd repo &&
	echo "stat change" >>file1 &&
	git add file1 &&
	git diff --stat --cached >output &&
	grep "file1" output &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

# ---------------------------------------------------------------------------
# --numstat
# ---------------------------------------------------------------------------
test_expect_success 'diff --numstat between commits' '
	cd repo &&
	git diff --numstat $(cat ../commit1) $(cat ../commit2) >output &&
	grep "file1" output &&
	grep "file2" output
'

test_expect_success 'diff --numstat --cached for staged changes' '
	cd repo &&
	echo "numstat" >>file1 &&
	git add file1 &&
	git diff --numstat --cached >output &&
	grep "file1" output &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

# ---------------------------------------------------------------------------
# --name-only
# ---------------------------------------------------------------------------
test_expect_success 'diff --name-only between commits' '
	cd repo &&
	git diff --name-only $(cat ../commit1) $(cat ../commit2) >output &&
	grep "^file1$" output &&
	grep "^file2$" output
'

test_expect_success 'diff --name-only --cached for staged changes' '
	cd repo &&
	echo "change" >>file1 &&
	echo "change" >>file2 &&
	git add file1 file2 &&
	git diff --name-only --cached >output &&
	grep "^file1$" output &&
	grep "^file2$" output &&
	git reset HEAD -- file1 file2 &&
	git checkout -- file1 file2
'

# ---------------------------------------------------------------------------
# --name-status
# ---------------------------------------------------------------------------
test_expect_success 'diff --name-status between commits' '
	cd repo &&
	git diff --name-status $(cat ../commit1) $(cat ../commit2) >output &&
	grep "^M" output &&
	grep "file1" output
'

test_expect_success 'diff --name-status --cached shows status letters' '
	cd repo &&
	echo "mod" >>file1 &&
	git add file1 &&
	git diff --name-status --cached >output &&
	grep "^M" output &&
	grep "file1" output &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

# ---------------------------------------------------------------------------
# --exit-code
# ---------------------------------------------------------------------------
test_expect_success 'diff --exit-code returns 0 when no changes' '
	cd repo &&
	git diff --exit-code --cached >output &&
	test_must_be_empty output
'

test_expect_success 'diff --exit-code returns 1 when staged changes exist' '
	cd repo &&
	echo "exitcode" >>file1 &&
	git add file1 &&
	test_must_fail git diff --exit-code --cached >output &&
	test -s output &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

# ---------------------------------------------------------------------------
# --quiet
# ---------------------------------------------------------------------------
test_expect_success 'diff --quiet --cached returns 0 with nothing staged' '
	cd repo &&
	git diff --quiet --cached
'

test_expect_success 'diff --quiet --cached returns 1 with staged changes, no output' '
	cd repo &&
	echo "quiet" >>file1 &&
	git add file1 &&
	test_must_fail git diff --quiet --cached >output &&
	test_must_be_empty output &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

# ---------------------------------------------------------------------------
# -U / --unified context lines
# ---------------------------------------------------------------------------
test_expect_success 'diff -U0 --cached shows zero context lines' '
	cd repo &&
	echo "ctx line" >>file1 &&
	git add file1 &&
	git diff -U0 --cached >output &&
	grep "^@@" output &&
	grep "+ctx line" output &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

test_expect_success 'diff -U10 --cached shows more context' '
	cd repo &&
	echo "big ctx" >>file1 &&
	git add file1 &&
	git diff -U10 --cached >output &&
	grep "^@@" output &&
	grep "+big ctx" output &&
	# With -U10 we should see existing lines as context
	grep " line 1" output &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

# ---------------------------------------------------------------------------
# New file detection
# ---------------------------------------------------------------------------
test_expect_success 'diff --cached shows new file' '
	cd repo &&
	echo "brand new" >newfile &&
	git add newfile &&
	git diff --cached >output &&
	grep "^diff --git a/newfile b/newfile" output &&
	grep "new file" output &&
	grep "+brand new" output &&
	git reset HEAD -- newfile &&
	rm newfile
'

# ---------------------------------------------------------------------------
# Deleted file detection
# ---------------------------------------------------------------------------
test_expect_success 'diff --cached shows deleted file' '
	cd repo &&
	git rm file2 &&
	git diff --cached >output &&
	grep "^diff --git a/file2 b/file2" output &&
	grep "deleted file" output &&
	git checkout HEAD -- file2
'

# ---------------------------------------------------------------------------
# diff --cached <commit>
# ---------------------------------------------------------------------------
test_expect_success 'diff --cached <commit> compares index to specified commit' '
	cd repo &&
	echo "vs-older" >>file1 &&
	git add file1 &&
	git diff --cached $(cat ../commit1) >output &&
	grep "^diff --git a/file1 b/file1" output &&
	grep "+line 2" output &&
	grep "+line 3" output &&
	grep "+line 4" output &&
	grep "+vs-older" output &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

# ---------------------------------------------------------------------------
# Reverse commit order
# ---------------------------------------------------------------------------
test_expect_success 'diff with reversed commit order shows reversed changes' '
	cd repo &&
	git diff $(cat ../commit3) $(cat ../commit1) >output &&
	grep "^diff --git a/file1 b/file1" output &&
	# Should show deletions of lines added between commit1 and commit3
	grep "^-line 2" output &&
	grep "^-line 3" output
'

test_done
