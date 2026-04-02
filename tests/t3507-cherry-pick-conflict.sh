#!/bin/sh
#
# Tests for 'grit cherry-pick' conflict scenarios — resolution, abort, skip.
# Ported subset from git/t/t3507-cherry-pick-conflict.sh (upstream ~44 tests).

test_description='grit cherry-pick — conflict handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup — a base repo with diverging branches
# ---------------------------------------------------------------------------
test_expect_success 'setup repository with conflicting branches' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "base content" >file &&
	git add file &&
	git commit -m "initial" &&
	git rev-parse HEAD >../initial &&

	git checkout -b side &&
	echo "side change" >file &&
	git add file &&
	git commit -m "side: modify file" &&
	git rev-parse HEAD >../side1 &&

	echo "side-only" >side-only.txt &&
	git add side-only.txt &&
	git commit -m "side: add new file" &&
	git rev-parse HEAD >../side2 &&

	git checkout master &&
	echo "master change" >file &&
	git add file &&
	git commit -m "master: modify file"
'

# ---------------------------------------------------------------------------
# Conflict detection
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick detects conflict' '
	cd repo &&
	test_must_fail git cherry-pick $(cat ../side1)
'

test_expect_success 'conflicted file contains conflict markers' '
	cd repo &&
	grep "<<<<<<" file &&
	grep ">>>>>>" file
'

test_expect_success 'CHERRY_PICK_HEAD is set during conflict' '
	cd repo &&
	test -f .git/CHERRY_PICK_HEAD &&
	head=$(cat .git/CHERRY_PICK_HEAD) &&
	test "$head" = "$(cat ../side1)"
'

test_expect_success 'MERGE_MSG is created during conflict' '
	cd repo &&
	test -f .git/MERGE_MSG &&
	grep "side: modify file" .git/MERGE_MSG
'

test_expect_success 'MERGE_MSG lists conflicted files' '
	cd repo &&
	grep "file" .git/MERGE_MSG
'

# ---------------------------------------------------------------------------
# Abort
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick --abort restores original state' '
	cd repo &&
	before=$(git rev-parse HEAD) &&
	git cherry-pick --abort &&
	after=$(git rev-parse HEAD) &&
	test "$before" = "$after"
'

test_expect_success 'CHERRY_PICK_HEAD removed after abort' '
	cd repo &&
	! test -f .git/CHERRY_PICK_HEAD
'

test_expect_success 'MERGE_MSG removed after abort' '
	cd repo &&
	! test -f .git/MERGE_MSG
'

test_expect_success 'working tree is clean after abort' '
	cd repo &&
	git status --porcelain >status.out &&
	! grep "^UU\|^AA\|^DD" status.out
'

# ---------------------------------------------------------------------------
# Skip
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick conflict then skip' '
	cd repo &&
	test_must_fail git cherry-pick $(cat ../side1) &&
	git cherry-pick --skip
'

test_expect_success 'skip removes CHERRY_PICK_HEAD' '
	cd repo &&
	! test -f .git/CHERRY_PICK_HEAD
'

test_expect_success 'skipped commit is not applied' '
	cd repo &&
	test "$(cat file)" = "master change"
'

# ---------------------------------------------------------------------------
# Non-conflicting cherry-pick
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick non-conflicting commit succeeds' '
	cd repo &&
	git cherry-pick $(cat ../side2) &&
	test -f side-only.txt &&
	test "$(cat side-only.txt)" = "side-only"
'

# ---------------------------------------------------------------------------
# Abort with no cherry-pick in progress
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick --abort with nothing in progress fails' '
	cd repo &&
	test_must_fail git cherry-pick --abort
'

# ---------------------------------------------------------------------------
# Cherry-pick from detached HEAD (use a fresh commit)
# ---------------------------------------------------------------------------
test_expect_success 'setup side3 for detached HEAD test' '
	cd repo &&
	git checkout side &&
	echo "extra" >extra.txt &&
	git add extra.txt &&
	git commit -m "side: add extra" &&
	git rev-parse HEAD >../side3 &&
	git checkout master
'

test_expect_success 'cherry-pick works from detached HEAD' '
	cd repo &&
	sha=$(git rev-parse master) &&
	git checkout "$sha" &&
	git cherry-pick $(cat ../side3) &&
	test -f extra.txt &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# Multiple conflict cycle: conflict, abort, re-apply
# ---------------------------------------------------------------------------
test_expect_success 'conflict then abort then retry' '
	cd repo &&
	test_must_fail git cherry-pick $(cat ../side1) &&
	git cherry-pick --abort &&
	test_must_fail git cherry-pick $(cat ../side1) &&
	git cherry-pick --abort
'

# ---------------------------------------------------------------------------
# Cherry-pick with file addition (no conflict)
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick adding new file does not conflict' '
	cd repo &&
	git checkout -f master &&
	git checkout -b fresh-pick &&
	git cherry-pick $(cat ../side3) &&
	test -f extra.txt &&
	test "$(cat extra.txt)" = "extra"
'

# ---------------------------------------------------------------------------
# Cherry-pick preserves commit message
# ---------------------------------------------------------------------------
test_expect_success 'cherry-pick preserves original commit message' '
	git init msg-repo &&
	cd msg-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo base >file &&
	git add file &&
	git commit -m "base" &&
	git checkout -b pick-src &&
	echo "pick-content" >pick.txt &&
	git add pick.txt &&
	git commit -m "the original message" &&
	pick_sha=$(git rev-parse HEAD) &&
	git checkout master &&
	git cherry-pick "$pick_sha" &&
	git log -n 1 --format=%s >msg.out &&
	grep "the original message" msg.out
'

test_done
