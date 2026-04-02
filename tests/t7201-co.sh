#!/bin/sh
#
# Tests for 'grit checkout' — branch switching, file restore, merge scenarios.
# Ported subset from git/t/t7201-co.sh (upstream ~46 tests).

test_description='grit checkout — branch switching and file restore'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository with branches' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "initial" >file &&
	git add file &&
	git commit -m "initial" &&

	git checkout -b side &&
	echo "side" >side-file &&
	git add side-file &&
	git commit -m "side commit" &&

	git checkout master &&
	echo "master-only" >master-file &&
	git add master-file &&
	git commit -m "master commit"
'

# ---------------------------------------------------------------------------
# Basic branch switching
# ---------------------------------------------------------------------------
test_expect_success 'checkout existing branch' '
	cd repo &&
	git checkout side &&
	test -f side-file &&
	! test -f master-file
'

test_expect_success 'checkout back to master' '
	cd repo &&
	git checkout master &&
	test -f master-file &&
	! test -f side-file
'

test_expect_success 'checkout -b creates new branch' '
	cd repo &&
	git checkout -b new-branch &&
	branch=$(git symbolic-ref HEAD) &&
	test "$branch" = "refs/heads/new-branch"
'

test_expect_success 'checkout -b from specific start point' '
	cd repo &&
	git checkout master &&
	start=$(git rev-parse master) &&
	git checkout -b from-start master &&
	current=$(git rev-parse HEAD) &&
	test "$current" = "$start"
'

test_expect_success 'checkout -B resets existing branch' '
	cd repo &&
	git checkout master &&
	git checkout -B new-branch &&
	current=$(git rev-parse HEAD) &&
	master=$(git rev-parse master) &&
	test "$current" = "$master"
'

# ---------------------------------------------------------------------------
# Detached HEAD
# ---------------------------------------------------------------------------
test_expect_success 'checkout by SHA detaches HEAD' '
	cd repo &&
	sha=$(git rev-parse master) &&
	git checkout "$sha" &&
	test_must_fail git symbolic-ref HEAD
'

test_expect_success 'checkout branch re-attaches HEAD' '
	cd repo &&
	git checkout master &&
	ref=$(git symbolic-ref HEAD) &&
	test "$ref" = "refs/heads/master"
'

# ---------------------------------------------------------------------------
# File restore (checkout -- path)
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- path restores file from index' '
	cd repo &&
	git checkout master &&
	echo "original" >file &&
	git add file &&
	git commit -m "set original" &&
	echo "dirty" >file &&
	git checkout -- file &&
	test "$(cat file)" = "original"
'

test_expect_success 'checkout -- path does not switch branch' '
	cd repo &&
	git checkout master &&
	echo "dirty" >file &&
	git checkout -- file &&
	ref=$(git symbolic-ref HEAD) &&
	test "$ref" = "refs/heads/master"
'

test_expect_success 'checkout -- path with multiple files' '
	cd repo &&
	echo "a" >a.txt &&
	echo "b" >b.txt &&
	git add a.txt b.txt &&
	git commit -m "add a and b" &&
	echo "dirty-a" >a.txt &&
	echo "dirty-b" >b.txt &&
	git checkout -- a.txt b.txt &&
	test "$(cat a.txt)" = "a" &&
	test "$(cat b.txt)" = "b"
'

# ---------------------------------------------------------------------------
# Dirty working tree checks
# ---------------------------------------------------------------------------
test_expect_success 'checkout refuses with dirty tracked file' '
	cd repo &&
	git checkout master &&
	git checkout -b dirty-test &&
	echo "change" >file &&
	git add file &&
	git commit -m "change on dirty-test" &&
	git checkout master &&
	echo "local-change" >file &&
	test_must_fail git checkout dirty-test
'

test_expect_success 'checkout -f forces switch with dirty tree' '
	cd repo &&
	echo "local-change" >file &&
	git checkout -f dirty-test &&
	ref=$(git symbolic-ref HEAD) &&
	test "$ref" = "refs/heads/dirty-test"
'

# ---------------------------------------------------------------------------
# Orphan branch
# ---------------------------------------------------------------------------
test_expect_success 'checkout --orphan creates parentless branch' '
	cd repo &&
	git checkout master &&
	git checkout --orphan orphan-branch &&
	ref=$(git symbolic-ref HEAD) &&
	test "$ref" = "refs/heads/orphan-branch" &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# Checkout with new files
# ---------------------------------------------------------------------------
test_expect_success 'untracked files survive branch switch' '
	cd repo &&
	git checkout -f master &&
	echo "untracked" >untracked.txt &&
	git checkout side &&
	test -f untracked.txt &&
	git checkout master &&
	rm -f untracked.txt
'

test_expect_success 'checkout -b preserves staged changes' '
	cd repo &&
	git checkout master &&
	echo "staged" >staged.txt &&
	git add staged.txt &&
	git checkout -b with-staged &&
	git status --porcelain >status.out &&
	grep "^A" status.out &&
	git checkout master &&
	git reset HEAD staged.txt 2>/dev/null;
	rm -f staged.txt
'

# ---------------------------------------------------------------------------
# Checkout specific commit paths
# ---------------------------------------------------------------------------
test_expect_success 'checkout path from different branch' '
	cd repo &&
	git checkout -f master &&
	git checkout side -- side-file &&
	test -f side-file
'

test_expect_success 'checkout nonexistent branch fails' '
	cd repo &&
	test_must_fail git checkout no-such-branch
'

test_expect_success 'checkout nonexistent path fails' '
	cd repo &&
	test_must_fail git checkout -- no-such-file
'

# ---------------------------------------------------------------------------
# Multiple sequential checkouts
# ---------------------------------------------------------------------------
test_expect_success 'rapid branch switching preserves content' '
	cd repo &&
	git checkout -f master &&
	git checkout side &&
	test -f side-file &&
	git checkout master &&
	test -f master-file &&
	git checkout side &&
	test -f side-file
'

test_expect_success 'checkout . restores all modified files' '
	cd repo &&
	git checkout -f master &&
	echo "dirty1" >file &&
	echo "dirty2" >master-file &&
	git checkout -- . &&
	test "$(cat master-file)" = "master-only"
'

test_done
