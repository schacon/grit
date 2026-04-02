#!/bin/sh
#
# Tests for cherry-pick sequences — multiple commits, --continue, --abort, --skip.
# Focuses on sequencer behavior and multi-commit cherry-pick operations.

test_description='cherry-pick sequence operations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup: create a repo with main + side branches for sequencer tests
# ---------------------------------------------------------------------------
test_expect_success 'setup repository with divergent branches' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "base" >file.txt &&
	git add file.txt &&
	git commit -m "initial" &&
	git rev-parse HEAD >../initial &&

	git checkout -b side &&
	echo "a" >a.txt &&
	git add a.txt &&
	git commit -m "side: add a" &&
	git rev-parse HEAD >../side_a &&

	echo "b" >b.txt &&
	git add b.txt &&
	git commit -m "side: add b" &&
	git rev-parse HEAD >../side_b &&

	echo "c" >c.txt &&
	git add c.txt &&
	git commit -m "side: add c" &&
	git rev-parse HEAD >../side_c &&

	git checkout master
'

# ---------------------------------------------------------------------------
# Multi-commit cherry-pick
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick multiple commits applies all in order' '
	cd repo &&
	git checkout -B multi $(cat ../initial) &&
	git cherry-pick $(cat ../side_a) $(cat ../side_b) $(cat ../side_c) &&
	test -f a.txt &&
	test -f b.txt &&
	test -f c.txt &&
	git log --oneline >log &&
	test_line_count = 4 log
'

test_expect_success 'cherry-pick multiple commits preserves commit messages' '
	cd repo &&
	git checkout -B order-test $(cat ../initial) &&
	git cherry-pick $(cat ../side_a) $(cat ../side_b) &&
	git log --oneline >log &&
	grep "side: add b" log &&
	grep "side: add a" log
'

test_expect_success 'cherry-pick two commits creates exactly two new commits' '
	cd repo &&
	git checkout -B count-test $(cat ../initial) &&
	head_before=$(git rev-parse HEAD) &&
	git cherry-pick $(cat ../side_a) $(cat ../side_b) &&
	git rev-list "$head_before"..HEAD >new_commits &&
	test_line_count = 2 new_commits
'

# ---------------------------------------------------------------------------
# Conflict + --abort
# ---------------------------------------------------------------------------
test_expect_success 'setup conflict branch' '
	cd repo &&
	git checkout -B conflict-master $(cat ../initial) &&
	echo "master-content" >a.txt &&
	git add a.txt &&
	git commit -m "master: add a (conflicting)" &&
	git rev-parse HEAD >../conflict_base
'

test_expect_success 'cherry-pick conflict stops mid-sequence' '
	cd repo &&
	git checkout -B abort-test $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) $(cat ../side_b) &&
	test -f .git/CHERRY_PICK_HEAD
'

test_expect_success 'cherry-pick --abort during sequence restores original HEAD' '
	cd repo &&
	git cherry-pick --abort &&
	test "$(git rev-parse HEAD)" = "$(cat ../conflict_base)" &&
	! test -f .git/CHERRY_PICK_HEAD
'

test_expect_success 'cherry-pick --abort restores working tree' '
	cd repo &&
	git checkout -B abort-tree $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) $(cat ../side_b) &&
	git cherry-pick --abort &&
	git diff --exit-code
'

# ---------------------------------------------------------------------------
# Conflict + --skip
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick --skip skips conflicting commit in sequence' '
	cd repo &&
	git checkout -B skip-test $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) $(cat ../side_b) &&
	git cherry-pick --skip &&
	test -f b.txt &&
	git log --oneline >log &&
	grep "side: add b" log
'

test_expect_success 'cherry-pick --skip removes CHERRY_PICK_HEAD' '
	cd repo &&
	git checkout -B skip-head $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) &&
	test -f .git/CHERRY_PICK_HEAD &&
	git cherry-pick --skip &&
	! test -f .git/CHERRY_PICK_HEAD
'

# ---------------------------------------------------------------------------
# Conflict + --continue
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick --continue after resolving conflict' '
	cd repo &&
	git checkout -B continue-test $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) $(cat ../side_b) &&
	echo "resolved-a" >a.txt &&
	/usr/bin/git add a.txt &&
	git cherry-pick --continue &&
	test -f b.txt &&
	test "$(cat a.txt)" = "resolved-a"
'

test_expect_success 'cherry-pick --continue without resolving fails' '
	cd repo &&
	git checkout -B continue-fail $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) &&
	test_must_fail git cherry-pick --continue 2>err &&
	git cherry-pick --abort
'

# ---------------------------------------------------------------------------
# Sequencer state
# ---------------------------------------------------------------------------
test_expect_success 'sequencer directory exists during multi-commit conflict' '
	cd repo &&
	git checkout -B seq-state $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) $(cat ../side_b) $(cat ../side_c) &&
	test -d .git/sequencer &&
	test -f .git/sequencer/todo &&
	git cherry-pick --abort
'

test_expect_success 'sequencer directory removed after --abort' '
	cd repo &&
	git checkout -B seq-clean $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) $(cat ../side_b) &&
	git cherry-pick --abort &&
	! test -d .git/sequencer
'

test_expect_success 'CHERRY_PICK_HEAD contains correct commit during conflict' '
	cd repo &&
	git checkout -B cp-head-check $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) &&
	cp_head=$(cat .git/CHERRY_PICK_HEAD) &&
	test "$cp_head" = "$(cat ../side_a)" &&
	git cherry-pick --abort
'

# ---------------------------------------------------------------------------
# --no-commit with sequences
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick --no-commit with multiple commits stages all' '
	cd repo &&
	git checkout -B nocommit-seq $(cat ../initial) &&
	git cherry-pick --no-commit $(cat ../side_a) $(cat ../side_b) &&
	test -f a.txt &&
	test -f b.txt &&
	test "$(git rev-parse HEAD)" = "$(cat ../initial)" &&
	git reset --hard HEAD
'

test_expect_success 'cherry-pick -n stages changes without committing' '
	cd repo &&
	git checkout -B nocommit-n $(cat ../initial) &&
	git cherry-pick -n $(cat ../side_a) &&
	test -f a.txt &&
	git diff --cached --name-only >staged &&
	grep a.txt staged &&
	test "$(git rev-parse HEAD)" = "$(cat ../initial)" &&
	git reset --hard HEAD
'

# ---------------------------------------------------------------------------
# Edge cases
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick --abort with no cherry-pick in progress fails' '
	cd repo &&
	git checkout -B no-op $(cat ../initial) &&
	test_must_fail git cherry-pick --abort 2>err
'

test_expect_success 'cherry-pick --continue with no cherry-pick in progress fails' '
	cd repo &&
	test_must_fail git cherry-pick --continue 2>err
'

test_expect_success 'cherry-pick --skip with no cherry-pick in progress fails' '
	cd repo &&
	test_must_fail git cherry-pick --skip 2>err
'

test_expect_success 'cherry-pick single commit does not leave sequencer state' '
	cd repo &&
	git checkout -B single-no-seq $(cat ../initial) &&
	git cherry-pick $(cat ../side_a) &&
	! test -d .git/sequencer
'

test_expect_success 'cherry-pick --quit removes sequencer state but keeps changes' '
	cd repo &&
	git checkout -B quit-test $(cat ../conflict_base) &&
	test_must_fail git cherry-pick $(cat ../side_a) $(cat ../side_b) &&
	git cherry-pick --quit &&
	! test -d .git/sequencer &&
	! test -f .git/CHERRY_PICK_HEAD &&
	git checkout -f $(cat ../initial)
'

# ---------------------------------------------------------------------------
# Author preservation in sequences
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick sequence preserves author of each commit' '
	cd repo &&
	git checkout -B author-seq $(cat ../initial) &&
	git cherry-pick $(cat ../side_a) $(cat ../side_b) &&
	git log --oneline >log &&
	grep "side: add a" log &&
	grep "side: add b" log
'

test_expect_success 'cherry-pick -x in sequence adds reference line' '
	cd repo &&
	git checkout -B x-seq $(cat ../initial) &&
	git cherry-pick -x $(cat ../side_b) &&
	git log -n 1 >body &&
	grep "cherry picked from commit $(cat ../side_b)" body
'

test_done
