#!/bin/sh
#
# Tests for 'grit cherry-pick' — applies changes from existing commits.
# cherry-pick is a passthrough command but we verify grit dispatches correctly.

test_description='grit cherry-pick — apply changes from existing commits'

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

	echo "base" >file1 &&
	git add file1 &&
	git commit -m "initial commit" &&
	git rev-parse HEAD >../initial
'

# ---------------------------------------------------------------------------
# Basic cherry-pick
# ---------------------------------------------------------------------------
test_expect_success 'setup feature branch with commits to cherry-pick' '
	cd repo &&
	git checkout -b feature &&
	echo "feature line" >>file1 &&
	git commit -a -m "feature: modify file1" &&
	git rev-parse HEAD >../feature1 &&

	echo "new file" >file2 &&
	git add file2 &&
	git commit -m "feature: add file2" &&
	git rev-parse HEAD >../feature2 &&

	echo "another line" >>file1 &&
	git commit -a -m "feature: another change" &&
	git rev-parse HEAD >../feature3 &&

	git checkout master
'

test_expect_success 'cherry-pick a single commit' '
	cd repo &&
	git cherry-pick $(cat ../feature2) &&
	test -f file2 &&
	test "$(cat file2)" = "new file"
'

test_expect_success 'cherry-pick creates a new commit (different SHA)' '
	cd repo &&
	head_sha=$(git rev-parse HEAD) &&
	test "$head_sha" != "$(cat ../feature2)"
'

test_expect_success 'cherry-picked commit has correct message' '
	cd repo &&
	git log -n 1 --format=%s >msg &&
	grep "feature: add file2" msg
'

test_expect_success 'cherry-picked commit has correct parent' '
	cd repo &&
	parent=$(git rev-parse HEAD~1) &&
	test "$parent" = "$(cat ../initial)"
'

# ---------------------------------------------------------------------------
# Cherry-pick with conflict
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick with conflict fails' '
	cd repo &&
	# feature1 modifies file1 line 2, master has only "base"
	# First modify file1 on master to create conflict
	echo "master line" >>file1 &&
	git commit -a -m "master: modify file1" &&

	test_must_fail git cherry-pick $(cat ../feature1) 2>err
'

test_expect_success 'cherry-pick --abort restores state' '
	cd repo &&
	git cherry-pick --abort &&
	# HEAD should be at the master commit
	git log -n 1 --format=%s >msg &&
	grep "master: modify file1" msg &&
	# Working tree should be clean
	git diff --exit-code &&
	git diff --cached --exit-code
'

# ---------------------------------------------------------------------------
# Cherry-pick with --no-commit
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick --no-commit stages changes without committing' '
	cd repo &&
	git reset --hard $(cat ../initial) &&
	git cherry-pick --no-commit $(cat ../feature2) &&
	# file2 should exist and be staged
	test -f file2 &&
	git diff --cached --name-only >staged &&
	grep "file2" staged &&
	# But no new commit should have been made
	test "$(git rev-parse HEAD)" = "$(cat ../initial)" &&
	git reset --hard HEAD &&
	rm -f file2
'

# ---------------------------------------------------------------------------
# Cherry-pick multiple commits
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick nonexistent commit fails' '
	cd repo &&
	git reset --hard $(cat ../initial) &&
	test_must_fail git cherry-pick deadbeefdeadbeefdeadbeefdeadbeefdeadbeef 2>err &&
	test -s err
'

# ---------------------------------------------------------------------------
# Cherry-pick onto empty-ish history
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick applies cleanly when no overlap' '
	cd repo &&
	git reset --hard $(cat ../initial) &&
	git cherry-pick $(cat ../feature2) &&
	test -f file2 &&
	test "$(cat file2)" = "new file" &&
	# file1 should still be just "base"
	test "$(cat file1)" = "base"
'

# ---------------------------------------------------------------------------
# Cherry-pick preserves author info
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick preserves original author' '
	cd repo &&
	original_author=$(git log -n 1 --format="%an <%ae>" $(cat ../feature2)) &&
	picked_author=$(git log -n 1 --format="%an <%ae>") &&
	test "$original_author" = "$picked_author"
'

# ---------------------------------------------------------------------------
# Cherry-pick with -x adds reference
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick -x adds cherry-picked-from line' '
	cd repo &&
	git reset --hard $(cat ../initial) &&
	git cherry-pick -x $(cat ../feature2) &&
	git log -n 1 --format=%b >body &&
	grep "cherry picked from commit $(cat ../feature2)" body
'

# ---------------------------------------------------------------------------
# Cherry-pick onto a new branch
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick onto a new branch' '
	cd repo &&
	git checkout -b pick-branch $(cat ../initial) &&
	git cherry-pick $(cat ../feature2) &&
	test -f file2 &&
	# Master should still not have the commit (at initial)
	git checkout master &&
	git reset --hard $(cat ../initial) &&
	test_path_is_missing file2
'

test_done
