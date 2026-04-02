#!/bin/sh
# Ported subset from git/t/t4013-diff-various.sh for diff-index -m behavior.

test_description='diff-index default vs -m for missing worktree files'

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

# ---------------------------------------------------------------------------
# Additional diff-various tests (from git/t/t4013 patterns)
# ---------------------------------------------------------------------------

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

# ---------------------------------------------------------------------------
# diff-tree and commit-to-commit diff tests (ported from git/t/t4013 patterns)
# ---------------------------------------------------------------------------

test_expect_success 'setup repo3 for commit-to-commit diff tests' '
	git init repo3 &&
	cd repo3 &&
	printf "alpha\nbeta\ngamma\n" >file.txt &&
	printf "first content\n" >readme.txt &&
	git update-index --add file.txt readme.txt &&
	c1=$(make_commit initial) &&
	printf "%s\n" "$c1" >c1 &&
	printf "alpha\nbeta\ngamma\ndelta\n" >file.txt &&
	printf "new file content\n" >newfile.txt &&
	git update-index --add file.txt newfile.txt &&
	c2=$(make_commit second "$c1") &&
	printf "%s\n" "$c2" >c2
'

test_expect_success 'diff-tree with two commits shows changes' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff-tree "$c1" "$c2" >out &&
	grep "file.txt" out &&
	grep "newfile.txt" out
'

test_expect_success 'diff-tree --name-only between two commits' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff-tree --name-only "$c1" "$c2" >out &&
	grep "^file.txt$" out &&
	grep "^newfile.txt$" out
'

test_expect_success 'diff-tree --name-status shows M and A' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff-tree --name-status "$c1" "$c2" >out &&
	grep "^M.*file.txt" out &&
	grep "^A.*newfile.txt" out
'

test_expect_success 'diff-tree -p shows patch between commits' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff-tree -p "$c1" "$c2" >out &&
	grep "^diff --git" out &&
	grep "^+delta" out
'

test_expect_success 'diff-tree --stat shows changed files' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff-tree --stat "$c1" "$c2" >out &&
	grep "file.txt" out &&
	grep "changed" out
'

test_expect_success 'diff-tree with pathspec limits to matching file' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff-tree --name-only "$c1" "$c2" -- file.txt >out &&
	grep "^file.txt$" out &&
	! grep "newfile.txt" out
'

test_expect_success 'diff between two commits shows unified diff' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff "$c1" "$c2" >out &&
	grep "^diff --git" out &&
	grep "^+delta" out
'

test_expect_success 'diff --name-only between two commits' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff --name-only "$c1" "$c2" >out &&
	grep "file.txt" out &&
	grep "newfile.txt" out
'

test_expect_success 'diff --stat between two commits' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff --stat "$c1" "$c2" >out &&
	grep "file.txt" out
'

test_expect_success 'diff --numstat between two commits' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff --numstat "$c1" "$c2" >out &&
	grep "file.txt" out &&
	grep "^[0-9]" out
'

test_expect_success 'diff with pathspec limits output to matching file' '
	cd repo3 &&
	c1=$(cat c1) c2=$(cat c2) &&
	git diff --name-only "$c1" "$c2" -- file.txt >out &&
	grep "^file.txt$" out &&
	! grep "newfile.txt" out
'

test_done
