#!/bin/sh
#
# Tests for 'grit checkout -f' — force checkout overwriting local changes.

test_description='grit checkout -f overwriting local changes'

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

	echo "base" >file1.txt &&
	echo "base2" >file2.txt &&
	mkdir -p sub &&
	echo "subfile" >sub/s.txt &&
	git add . &&
	git commit -m "initial" &&
	git rev-parse HEAD >../initial_commit &&

	git checkout -b other &&
	echo "other" >file1.txt &&
	echo "other2" >file2.txt &&
	echo "other-sub" >sub/s.txt &&
	echo "other-new" >other-only.txt &&
	git add . &&
	git commit -m "other branch" &&

	git checkout master
'

# ---------------------------------------------------------------------------
# -f discards unstaged worktree changes
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f discards dirty worktree file' '
	cd repo &&
	echo "dirty" >file1.txt &&
	git checkout -f master &&
	test "$(cat file1.txt)" = "base"
'

test_expect_success 'checkout -f discards multiple dirty files' '
	cd repo &&
	echo "d1" >file1.txt &&
	echo "d2" >file2.txt &&
	echo "d3" >sub/s.txt &&
	git checkout -f master &&
	test "$(cat file1.txt)" = "base" &&
	test "$(cat file2.txt)" = "base2" &&
	test "$(cat sub/s.txt)" = "subfile"
'

# ---------------------------------------------------------------------------
# -f discards staged changes
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f discards staged changes' '
	cd repo &&
	echo "staged" >file1.txt &&
	git add file1.txt &&
	git checkout -f master &&
	test "$(cat file1.txt)" = "base" &&
	git diff --cached --exit-code
'

test_expect_success 'checkout -f discards staged new file' '
	cd repo &&
	echo "new-staged" >new-file.txt &&
	git add new-file.txt &&
	git checkout -f master &&
	git diff --cached --name-only >staged &&
	! grep new-file.txt staged
'

# ---------------------------------------------------------------------------
# -f with branch switching
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f to other branch with conflicting dirty file' '
	cd repo &&
	echo "dirty-conflict" >file1.txt &&
	git checkout -f other &&
	test "$(cat file1.txt)" = "other" &&
	test "$(git symbolic-ref --short HEAD)" = "other" &&
	git checkout -f master
'

test_expect_success 'checkout -f to branch brings new files' '
	cd repo &&
	git checkout -f other &&
	test -f other-only.txt &&
	test "$(cat other-only.txt)" = "other-new" &&
	git checkout -f master
'

test_expect_success 'checkout -f to master removes other-branch files' '
	cd repo &&
	git checkout -f other &&
	git checkout -f master &&
	test_path_is_missing other-only.txt
'

# ---------------------------------------------------------------------------
# -f with staged conflicting changes allows switch
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f with staged conflicting changes switches' '
	cd repo &&
	echo "staged-conflict" >file1.txt &&
	git add file1.txt &&
	git checkout -f other &&
	test "$(cat file1.txt)" = "other" &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# -f cleans up index to match target
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f leaves clean index' '
	cd repo &&
	echo "dirty" >file1.txt &&
	echo "staged" >file2.txt &&
	git add file2.txt &&
	git checkout -f other &&
	git diff --cached --exit-code &&
	git diff --exit-code &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# -f to same branch resets worktree
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f to same branch resets worktree' '
	cd repo &&
	echo "dirty" >file1.txt &&
	git checkout -f master &&
	test "$(cat file1.txt)" = "base"
'

test_expect_success 'checkout -f to same branch resets staged changes' '
	cd repo &&
	echo "staged" >file1.txt &&
	git add file1.txt &&
	git checkout -f master &&
	test "$(cat file1.txt)" = "base" &&
	git diff --cached --exit-code
'

# ---------------------------------------------------------------------------
# -f with deleted files
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f restores deleted file' '
	cd repo &&
	rm -f file1.txt &&
	git checkout -f master &&
	test -f file1.txt &&
	test "$(cat file1.txt)" = "base"
'

test_expect_success 'checkout -f restores git-rm deleted file' '
	cd repo &&
	git rm file1.txt &&
	git checkout -f master &&
	test -f file1.txt
'

# ---------------------------------------------------------------------------
# -f preserves untracked files
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f does not remove untracked files' '
	cd repo &&
	echo "untracked" >untracked.txt &&
	git checkout -f other &&
	test -f untracked.txt &&
	git checkout -f master &&
	rm -f untracked.txt
'

# ---------------------------------------------------------------------------
# -f to detached HEAD
# ---------------------------------------------------------------------------
test_expect_failure 'checkout -f to commit (detached HEAD)' '
	cd repo &&
	echo "dirty" >file1.txt &&
	git checkout -f $(cat ../initial_commit) 2>/dev/null &&
	test "$(cat file1.txt)" = "base" &&
	test_must_fail git symbolic-ref HEAD 2>/dev/null &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# -f with directory structure changes
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f handles subdirectory content changes' '
	cd repo &&
	echo "dirty-sub" >sub/s.txt &&
	git checkout -f other &&
	test "$(cat sub/s.txt)" = "other-sub" &&
	git checkout -f master &&
	test "$(cat sub/s.txt)" = "subfile"
'

# ---------------------------------------------------------------------------
# -f after adding files to index that are not on target
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f clears staged files absent on target' '
	cd repo &&
	echo "extra" >extra.txt &&
	git add extra.txt &&
	git checkout -f other &&
	git diff --cached --name-only >staged &&
	! grep extra.txt staged &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# checkout without -f refuses, -f succeeds (contrast test)
# ---------------------------------------------------------------------------
test_expect_success 'checkout refuses without -f, succeeds with -f' '
	cd repo &&
	echo "conflict" >file1.txt &&
	test_must_fail git checkout other 2>err &&
	test -s err &&
	git checkout -f other &&
	test "$(cat file1.txt)" = "other" &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# -f with -b (force create + force checkout)
# ---------------------------------------------------------------------------
test_expect_failure 'checkout -f -b creates branch discarding changes' '
	cd repo &&
	echo "dirty" >file1.txt &&
	git checkout -f -b force-new &&
	test "$(cat file1.txt)" = "base" &&
	test "$(git symbolic-ref --short HEAD)" = "force-new" &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# Rapid force checkout cycles
# ---------------------------------------------------------------------------
test_expect_success 'rapid -f switching between branches' '
	cd repo &&
	git checkout -f other &&
	git checkout -f master &&
	git checkout -f other &&
	git checkout -f master &&
	test "$(cat file1.txt)" = "base" &&
	test "$(git symbolic-ref --short HEAD)" = "master"
'

# ---------------------------------------------------------------------------
# -f cleans up worktree completely
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f with both staged and unstaged dirt' '
	cd repo &&
	echo "staged" >file1.txt &&
	git add file1.txt &&
	echo "further" >file1.txt &&
	echo "staged2" >file2.txt &&
	git add file2.txt &&
	rm -f sub/s.txt &&
	git checkout -f master &&
	test "$(cat file1.txt)" = "base" &&
	test "$(cat file2.txt)" = "base2" &&
	test -f sub/s.txt
'

test_done
