#!/bin/sh
#
# Tests for 'grit reset' in a bare repository.

test_description='grit reset in bare repository'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup: create a normal repo, then clone --bare via system git
# ---------------------------------------------------------------------------
test_expect_success 'setup bare repository' '
	git init source &&
	cd source &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "file-a" >a &&
	git add a &&
	git commit -m "c1" &&
	git rev-parse HEAD >../sha1 &&

	echo "file-b" >b &&
	git add b &&
	git commit -m "c2" &&
	git rev-parse HEAD >../sha2 &&

	echo "file-c" >c &&
	git add c &&
	git commit -m "c3" &&
	git rev-parse HEAD >../sha3 &&

	echo "file-d" >d &&
	git add d &&
	git commit -m "c4" &&
	git rev-parse HEAD >../sha4 &&

	cd .. &&
	git init --bare bare.git &&
	cd source &&
	GIT_DIR=../bare.git git update-ref refs/heads/master $(git rev-parse HEAD) &&
	cp -r .git/objects/* ../bare.git/objects/ &&
	cd ..
'

# ---------------------------------------------------------------------------
# Verify initial state
# ---------------------------------------------------------------------------
test_expect_success 'bare repo starts at c4' '
	cd bare.git &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha4)" &&
	test "$(git log --oneline | wc -l | tr -d " ")" = "4"
'

# ---------------------------------------------------------------------------
# reset --soft in bare repo
# ---------------------------------------------------------------------------
test_expect_success 'reset --soft HEAD~1 moves HEAD back one' '
	cd bare.git &&
	git reset --soft HEAD~1 &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha3)"
'

test_expect_success 'reset --soft to specific SHA' '
	cd bare.git &&
	git reset --soft $(cat ../sha1) &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha1)"
'

test_expect_success 'reset --soft forward to newer commit' '
	cd bare.git &&
	git reset --soft $(cat ../sha4) &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha4)"
'

# ---------------------------------------------------------------------------
# reset (mixed/default) in bare repo
# ---------------------------------------------------------------------------
test_expect_success 'reset (default/mixed) HEAD~1' '
	cd bare.git &&
	git reset HEAD~1 &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha3)"
'

test_expect_success 'reset (default) to specific SHA' '
	cd bare.git &&
	git reset $(cat ../sha1) &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha1)"
'

test_expect_success 'reset (default) forward' '
	cd bare.git &&
	git reset $(cat ../sha4) &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha4)"
'

# ---------------------------------------------------------------------------
# reset --hard in bare repo
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard HEAD~1' '
	cd bare.git &&
	git reset --hard HEAD~1 &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha3)"
'

test_expect_success 'reset --hard to specific SHA' '
	cd bare.git &&
	git reset --hard $(cat ../sha1) &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha1)"
'

test_expect_success 'reset --hard forward' '
	cd bare.git &&
	git reset --hard $(cat ../sha4) &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha4)"
'

# ---------------------------------------------------------------------------
# reset --soft HEAD (no-op)
# ---------------------------------------------------------------------------
test_expect_success 'reset --soft HEAD is a no-op' '
	cd bare.git &&
	before=$(git rev-parse HEAD) &&
	git reset --soft HEAD &&
	test "$(git rev-parse HEAD)" = "$before"
'

# ---------------------------------------------------------------------------
# reset HEAD~N with larger N
# ---------------------------------------------------------------------------
test_expect_success 'reset --soft HEAD~3 moves back three' '
	cd bare.git &&
	git reset --soft $(cat ../sha4) &&
	git reset --soft HEAD~3 &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha1)"
'

# ---------------------------------------------------------------------------
# Verify symbolic-ref still works after reset
# ---------------------------------------------------------------------------
test_expect_success 'symbolic-ref HEAD still points to master after reset' '
	cd bare.git &&
	test "$(git symbolic-ref HEAD)" = "refs/heads/master"
'

# ---------------------------------------------------------------------------
# reset updates branch ref, not just HEAD
# ---------------------------------------------------------------------------
test_expect_success 'reset updates branch tip (master matches HEAD)' '
	cd bare.git &&
	git reset --soft $(cat ../sha3) &&
	test "$(git rev-parse master)" = "$(cat ../sha3)" &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha3)"
'

# ---------------------------------------------------------------------------
# log shows correct history after reset
# ---------------------------------------------------------------------------
test_expect_success 'log shows shortened history after reset' '
	cd bare.git &&
	git reset --soft $(cat ../sha2) &&
	test "$(git log --oneline | wc -l | tr -d " ")" = "2"
'

# ---------------------------------------------------------------------------
# Tree contents are still accessible after reset
# ---------------------------------------------------------------------------
test_expect_success 'old commit tree is still accessible after reset' '
	cd bare.git &&
	git cat-file -t $(cat ../sha4) >type &&
	test "$(cat type)" = "commit"
'

test_expect_success 'cat-file shows commit content from old commit' '
	cd bare.git &&
	git cat-file -p $(cat ../sha4) >content &&
	grep "c4" content
'

# ---------------------------------------------------------------------------
# reset --hard HEAD restores to current state (idempotent)
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard HEAD is idempotent' '
	cd bare.git &&
	git reset --soft $(cat ../sha4) &&
	before=$(git rev-parse HEAD) &&
	git reset --hard HEAD &&
	test "$(git rev-parse HEAD)" = "$before"
'

# ---------------------------------------------------------------------------
# reset in bare does not create working tree files
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard in bare does not create worktree files' '
	cd bare.git &&
	git reset --hard $(cat ../sha4) &&
	test_path_is_missing a &&
	test_path_is_missing b &&
	test_path_is_missing c &&
	test_path_is_missing d
'

# ---------------------------------------------------------------------------
# Multiple resets in sequence
# ---------------------------------------------------------------------------
test_expect_success 'multiple sequential resets work correctly' '
	cd bare.git &&
	git reset --soft $(cat ../sha4) &&
	git reset --soft $(cat ../sha3) &&
	git reset --soft $(cat ../sha2) &&
	git reset --soft $(cat ../sha1) &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha1)" &&
	git reset --soft $(cat ../sha4) &&
	test "$(git rev-parse HEAD)" = "$(cat ../sha4)"
'

# ---------------------------------------------------------------------------
# Branch operations still work after reset
# ---------------------------------------------------------------------------
test_expect_success 'can create branch after reset in bare' '
	cd bare.git &&
	git reset --soft $(cat ../sha2) &&
	git branch new-branch $(cat ../sha3) &&
	test "$(git rev-parse new-branch)" = "$(cat ../sha3)"
'

test_expect_success 'can delete branch after reset in bare' '
	cd bare.git &&
	git branch -D new-branch &&
	test_must_fail git rev-parse new-branch 2>/dev/null
'

test_done
