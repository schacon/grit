#!/bin/sh
#
# Tests for branch creation/deletion via checkout -b/-B/-f.
# Adapted from git/t/t2018-checkout-branch.sh

test_description='checkout -b/-B branch creation and switching'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo initial >file1 &&
	git add file1 &&
	git commit -m "initial" &&
	git rev-parse HEAD >../head1 &&

	echo change1 >file1 &&
	git commit -a -m "change1" &&
	git rev-parse HEAD >../head2 &&

	git branch -m master branch1
'

# ---------------------------------------------------------------------------
# checkout -b creates and switches to new branch at HEAD
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b to a new branch, set to HEAD' '
	cd repo &&
	git checkout -b branch2 &&
	echo refs/heads/branch2 >ref.expect &&
	git symbolic-ref HEAD >ref.actual &&
	test_cmp ref.expect ref.actual &&
	cat ../head2 >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual &&
	git checkout branch1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -b to a new branch at an explicit ref
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b to a new branch at explicit ref' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	git checkout -b branch2 "$HEAD1" &&
	echo "$HEAD1" >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual &&
	git checkout branch1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -b with unmergeable changes fails
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b with unmergeable changes fails' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	echo dirty >>file1 &&
	test_must_fail git checkout -b branch2 "$HEAD1" &&
	git checkout -- file1
'

# ---------------------------------------------------------------------------
# checkout -f -b discards unmergeable changes
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f -b discards unmergeable changes' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	echo dirty >>file1 &&
	git checkout -f -b branch2 "$HEAD1" &&
	git diff --exit-code &&
	echo "$HEAD1" >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual &&
	git checkout branch1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -b preserves mergeable (staged, non-conflicting) changes
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b preserves mergeable changes' '
	cd repo &&
	echo newfile >file2 &&
	git add file2 &&
	git checkout -b branch2 &&
	git diff --cached --name-only >staged &&
	grep file2 staged &&
	git reset --hard &&
	git checkout branch1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -f -b discards mergeable changes
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f -b discards mergeable changes' '
	cd repo &&
	echo newfile >file2 &&
	git add file2 &&
	git checkout -f -b branch2 &&
	git diff --cached --name-only >staged &&
	test_must_be_empty staged &&
	git checkout branch1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -b to existing branch fails
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b to an existing branch fails' '
	cd repo &&
	git branch existing &&
	test_must_fail git checkout -b existing &&
	git branch -d existing
'

# ---------------------------------------------------------------------------
# checkout -B resets existing branch to HEAD
# ---------------------------------------------------------------------------
test_expect_success 'checkout -B resets existing branch to HEAD' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	HEAD2=$(cat ../head2) &&
	git branch branch2 "$HEAD1" &&
	old_oid=$(git rev-parse branch2) &&
	test "$old_oid" = "$HEAD1" &&
	git checkout -B branch2 &&
	new_oid=$(git rev-parse HEAD) &&
	test "$new_oid" = "$HEAD2" &&
	test "$old_oid" != "$new_oid" &&
	git checkout branch1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -B to existing branch with explicit ref
# ---------------------------------------------------------------------------
test_expect_success 'checkout -B to existing branch with explicit ref' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	git branch branch2 &&
	git checkout -B branch2 "$HEAD1" &&
	echo "$HEAD1" >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual &&
	git checkout branch1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -B from detached HEAD
# ---------------------------------------------------------------------------
test_expect_success 'checkout -B from detached HEAD resets branch' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	HEAD2=$(cat ../head2) &&
	git branch branch2 "$HEAD1" &&
	git checkout "$HEAD2" &&
	git checkout -B branch2 &&
	echo "$HEAD2" >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual &&
	echo refs/heads/branch2 >ref.expect &&
	git symbolic-ref HEAD >ref.actual &&
	test_cmp ref.expect ref.actual &&
	git checkout branch1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -B with unmergeable changes fails
# ---------------------------------------------------------------------------
test_expect_success 'checkout -B with unmergeable changes fails' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	git branch branch2 &&
	echo dirty >>file1 &&
	test_must_fail git checkout -B branch2 "$HEAD1" &&
	git checkout -- file1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -f -B discards unmergeable changes
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f -B discards unmergeable changes' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	git branch branch2 &&
	echo dirty >>file1 &&
	git checkout -f -B branch2 "$HEAD1" &&
	git diff --exit-code &&
	echo "$HEAD1" >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual &&
	git checkout branch1 &&
	git branch -D branch2
'

# ---------------------------------------------------------------------------
# checkout -B to the current branch works
# ---------------------------------------------------------------------------
test_expect_success 'checkout -B to the current branch works' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	git checkout -B branch1-scratch &&
	echo newfile >file2 &&
	git add file2 &&
	git checkout -B branch1-scratch "$HEAD1" &&
	git diff --cached --name-only >staged &&
	grep file2 staged &&
	git reset --hard &&
	git checkout branch1 &&
	git branch -D branch1-scratch
'

# ---------------------------------------------------------------------------
# checkout -b rejects invalid start point
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b rejects invalid start point' '
	cd repo &&
	test_must_fail git checkout -b branch4 file1 2>err &&
	test -s err
'

# ---------------------------------------------------------------------------
# checkout -b rejects extra path argument
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b rejects extra path argument' '
	cd repo &&
	test_must_fail git checkout -b branch5 branch1 file1 2>err &&
	test -s err
'

# ---------------------------------------------------------------------------
# checkout -b with --track sets upstream
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b --track sets upstream config' '
	cd repo &&
	git checkout -b tracked-branch --track branch1 &&
	test "$(git config branch.tracked-branch.merge)" = "refs/heads/branch1" &&
	git checkout branch1 &&
	git branch -D tracked-branch
'

# ---------------------------------------------------------------------------
# checkout -B creates branch if it does not exist
# ---------------------------------------------------------------------------
test_expect_success 'checkout -B creates new branch when it does not exist' '
	cd repo &&
	git checkout -B brand-new-B &&
	echo refs/heads/brand-new-B >ref.expect &&
	git symbolic-ref HEAD >ref.actual &&
	test_cmp ref.expect ref.actual &&
	git checkout branch1 &&
	git branch -D brand-new-B
'

# ---------------------------------------------------------------------------
# checkout -b from detached HEAD
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b from detached HEAD creates branch at detached commit' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	git checkout "$HEAD1" &&
	git checkout -b from-detached &&
	echo refs/heads/from-detached >ref.expect &&
	git symbolic-ref HEAD >ref.actual &&
	test_cmp ref.expect ref.actual &&
	echo "$HEAD1" >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual &&
	git checkout branch1 &&
	git branch -D from-detached
'

# ---------------------------------------------------------------------------
# checkout -b with tag as start point
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b from tag' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	git tag test-tag "$HEAD1" &&
	git checkout -b from-tag test-tag &&
	echo "$HEAD1" >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual &&
	git checkout branch1 &&
	git branch -D from-tag &&
	git tag -d test-tag
'

# ---------------------------------------------------------------------------
# checkout -B resets to explicit ref even when on that branch
# ---------------------------------------------------------------------------
test_expect_success 'checkout -B on current branch resets HEAD' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	HEAD2=$(cat ../head2) &&
	git checkout -B branch1 "$HEAD1" &&
	echo "$HEAD1" >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual &&
	git checkout -B branch1 "$HEAD2" &&
	echo "$HEAD2" >oid.expect &&
	git rev-parse HEAD >oid.actual &&
	test_cmp oid.expect oid.actual
'

# ---------------------------------------------------------------------------
# checkout -b preserves untracked files
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b preserves untracked files' '
	cd repo &&
	echo untracked >untracked-file &&
	git checkout -b untracked-test &&
	test -f untracked-file &&
	test "$(cat untracked-file)" = "untracked" &&
	git checkout branch1 &&
	git branch -D untracked-test &&
	rm -f untracked-file
'

# ---------------------------------------------------------------------------
# checkout -f -B discards mergeable changes
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f -B discards mergeable changes' '
	cd repo &&
	echo newfile >file2 &&
	git add file2 &&
	git checkout -f -B force-B-test &&
	git diff --cached --name-only >staged &&
	test_must_be_empty staged &&
	git checkout branch1 &&
	git branch -D force-B-test
'

# ---------------------------------------------------------------------------
# checkout -b then -b again with different start points
# ---------------------------------------------------------------------------
test_expect_success 'consecutive -b creations at different start points' '
	cd repo &&
	HEAD1=$(cat ../head1) &&
	HEAD2=$(cat ../head2) &&
	git checkout -b seq1 "$HEAD1" &&
	test "$(git rev-parse HEAD)" = "$HEAD1" &&
	git checkout -b seq2 "$HEAD2" &&
	test "$(git rev-parse HEAD)" = "$HEAD2" &&
	git checkout branch1 &&
	git branch -D seq1 seq2
'

# ---------------------------------------------------------------------------
# checkout -b does not modify reflog of other branches
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b does not clobber other branch refs' '
	cd repo &&
	HEAD2=$(cat ../head2) &&
	branch1_before=$(git rev-parse branch1) &&
	git checkout -b no-clobber "$HEAD2" &&
	branch1_after=$(git rev-parse branch1) &&
	test "$branch1_before" = "$branch1_after" &&
	git checkout branch1 &&
	git branch -D no-clobber
'

test_done
