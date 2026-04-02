#!/bin/sh
#
# Tests for detached HEAD scenarios via checkout.
# Adapted from git/t/t2020-checkout-detach.sh

test_description='checkout into detached HEAD state'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

check_detached () {
	test_must_fail git symbolic-ref -q HEAD >/dev/null
}

check_not_detached () {
	git symbolic-ref -q HEAD >/dev/null
}

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	test_commit one &&
	test_commit two &&
	test_commit three &&
	git tag -d three &&
	test_commit four &&
	git tag -d four &&
	git branch branch &&
	git tag tag
'

# ---------------------------------------------------------------------------
# Basic attach/detach checks
# ---------------------------------------------------------------------------
test_expect_success 'checkout branch does not detach' '
	cd repo &&
	git checkout master &&
	check_not_detached &&
	git checkout branch &&
	check_not_detached
'

test_expect_success 'checkout HEAD is a no-op, does not detach' '
	cd repo &&
	git checkout master &&
	cat .git/HEAD >expect &&
	git checkout HEAD &&
	cat .git/HEAD >actual &&
	check_not_detached &&
	test_cmp expect actual
'

test_expect_success 'checkout tag detaches' '
	cd repo &&
	git checkout master &&
	git checkout tag &&
	check_detached
'

test_expect_success 'checkout branch by full name detaches' '
	cd repo &&
	git checkout master &&
	git checkout refs/heads/branch &&
	check_detached
'

test_expect_success 'checkout non-ref detaches' '
	cd repo &&
	git checkout master &&
	git checkout branch^ &&
	check_detached
'

test_expect_success 'checkout ref^0 detaches' '
	cd repo &&
	git checkout master &&
	git checkout branch^0 &&
	check_detached
'

# ---------------------------------------------------------------------------
# --detach flag
# ---------------------------------------------------------------------------
test_expect_success 'checkout --detach detaches' '
	cd repo &&
	git checkout master &&
	git checkout --detach branch &&
	check_detached
'

test_expect_success 'checkout --detach without branch name' '
	cd repo &&
	git checkout master &&
	git checkout --detach &&
	check_detached
'

test_expect_success 'checkout --detach errors out for non-commit' '
	cd repo &&
	git checkout master &&
	check_not_detached &&
	test_must_fail git checkout --detach one^{tree} &&
	check_not_detached
'

test_expect_success 'checkout --detach errors out for extra argument' '
	cd repo &&
	git checkout master &&
	check_not_detached &&
	test_must_fail git checkout --detach tag one.t &&
	check_not_detached
'

test_expect_success 'checkout --detach and -b are incompatible' '
	cd repo &&
	git checkout master &&
	check_not_detached &&
	test_must_fail git checkout --detach -b newbranch tag &&
	check_not_detached
'

# ---------------------------------------------------------------------------
# --detach moves HEAD
# ---------------------------------------------------------------------------
test_expect_success 'checkout --detach moves HEAD' '
	cd repo &&
	git checkout one &&
	git checkout --detach two &&
	check_detached &&
	git diff --exit-code HEAD &&
	git diff --exit-code two
'

# ---------------------------------------------------------------------------
# Detach and re-attach
# ---------------------------------------------------------------------------
test_expect_success 'can re-attach after detach' '
	cd repo &&
	git checkout master &&
	git checkout --detach &&
	check_detached &&
	git checkout master &&
	check_not_detached
'

test_expect_success 'checkout branch after detached tag re-attaches' '
	cd repo &&
	git checkout tag &&
	check_detached &&
	git checkout master &&
	check_not_detached
'

# ---------------------------------------------------------------------------
# Detached HEAD at various points
# ---------------------------------------------------------------------------
test_expect_success 'detached HEAD at tag shows correct commit' '
	cd repo &&
	git checkout tag &&
	check_detached &&
	tag_oid=$(git rev-parse tag) &&
	head_oid=$(git rev-parse HEAD) &&
	test "$tag_oid" = "$head_oid"
'

test_expect_success 'detached HEAD at branch^ shows parent commit' '
	cd repo &&
	git checkout master &&
	parent=$(git rev-parse master^) &&
	git checkout master^ &&
	check_detached &&
	head_oid=$(git rev-parse HEAD) &&
	test "$parent" = "$head_oid"
'

# ---------------------------------------------------------------------------
# Commit on detached HEAD, then switch back
# ---------------------------------------------------------------------------
test_expect_success 'commit on detached HEAD creates orphan commit' '
	cd repo &&
	git checkout --detach two &&
	echo content >orphan-file &&
	git add orphan-file &&
	git commit -m "orphan commit" &&
	orphan_oid=$(git rev-parse HEAD) &&
	git checkout master 2>stderr &&
	check_not_detached &&
	# The orphan commit should still be resolvable
	git cat-file -t "$orphan_oid" >type &&
	echo commit >expect &&
	test_cmp expect type
'

# ---------------------------------------------------------------------------
# --show-current is empty in detached state
# ---------------------------------------------------------------------------
test_expect_success 'branch --show-current is empty when detached' '
	cd repo &&
	git checkout --detach &&
	git branch --show-current >actual &&
	test_must_be_empty actual
'

test_expect_success 'branch --show-current shows branch when attached' '
	cd repo &&
	git checkout master &&
	echo master >expect &&
	git branch --show-current >actual &&
	test_cmp expect actual
'

# ---------------------------------------------------------------------------
# Detach, checkout -, re-attach
# ---------------------------------------------------------------------------
test_expect_success 'checkout - re-attaches from detached state' '
	cd repo &&
	git checkout master &&
	git checkout --detach &&
	check_detached &&
	git checkout - &&
	check_not_detached &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ---------------------------------------------------------------------------
# Deepened: detach at HEAD~N
# ---------------------------------------------------------------------------
test_expect_success 'checkout HEAD~1 detaches at parent' '
	cd repo &&
	git checkout master &&
	parent=$(git rev-parse master~1) &&
	git checkout HEAD~1 &&
	check_detached &&
	test "$(git rev-parse HEAD)" = "$parent"
'

test_expect_success 'checkout HEAD~2 detaches at grandparent' '
	cd repo &&
	git checkout master &&
	gp=$(git rev-parse master~2) &&
	git checkout HEAD~2 &&
	check_detached &&
	test "$(git rev-parse HEAD)" = "$gp"
'

# ---------------------------------------------------------------------------
# Deepened: --detach with tag
# ---------------------------------------------------------------------------
test_expect_success 'checkout --detach tag detaches at tag commit' '
	cd repo &&
	git checkout master &&
	git checkout --detach tag &&
	check_detached &&
	tag_oid=$(git rev-parse tag) &&
	test "$(git rev-parse HEAD)" = "$tag_oid"
'

# ---------------------------------------------------------------------------
# Deepened: detach does not lose working tree changes that are compatible
# ---------------------------------------------------------------------------
test_expect_success 'checkout --detach preserves untracked files' '
	cd repo &&
	git checkout master &&
	echo untracked >untracked-det &&
	git checkout --detach &&
	check_detached &&
	test -f untracked-det &&
	rm -f untracked-det
'

# ---------------------------------------------------------------------------
# Deepened: multiple detach hops
# ---------------------------------------------------------------------------
test_expect_success 'detach -> detach at different commits' '
	cd repo &&
	git checkout master &&
	one_oid=$(git rev-parse one) &&
	two_oid=$(git rev-parse two) &&
	git checkout "$one_oid" &&
	check_detached &&
	test "$(git rev-parse HEAD)" = "$one_oid" &&
	git checkout "$two_oid" &&
	check_detached &&
	test "$(git rev-parse HEAD)" = "$two_oid"
'

# ---------------------------------------------------------------------------
# Deepened: HEAD contents in detached vs attached
# ---------------------------------------------------------------------------
test_expect_success 'HEAD file contains raw SHA when detached' '
	cd repo &&
	git checkout master &&
	git checkout --detach &&
	head_content=$(cat .git/HEAD) &&
	# Should be a hex SHA, not a ref
	echo "$head_content" | grep -E "^[0-9a-f]{40}$"
'

test_expect_success 'HEAD file contains ref when attached' '
	cd repo &&
	git checkout master &&
	head_content=$(cat .git/HEAD) &&
	echo "$head_content" | grep "^ref: refs/heads/master$"
'

# ---------------------------------------------------------------------------
# Deepened: checkout --detach from already-detached
# ---------------------------------------------------------------------------
test_expect_success 'checkout --detach when already detached is a no-op' '
	cd repo &&
	git checkout --detach two &&
	check_detached &&
	two_oid=$(git rev-parse two) &&
	test "$(git rev-parse HEAD)" = "$two_oid" &&
	git checkout --detach &&
	check_detached &&
	test "$(git rev-parse HEAD)" = "$two_oid"
'

# ---------------------------------------------------------------------------
# Deepened: checkout -b from detached creates branch
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b from detached HEAD creates branch there' '
	cd repo &&
	git checkout --detach two &&
	check_detached &&
	two_oid=$(git rev-parse two) &&
	git checkout -b from-detach-test &&
	check_not_detached &&
	test "$(git rev-parse HEAD)" = "$two_oid" &&
	git checkout master &&
	git branch -D from-detach-test
'

# ---------------------------------------------------------------------------
# Deepened: status in detached mode
# ---------------------------------------------------------------------------
test_expect_success 'status works in detached HEAD' '
	cd repo &&
	git checkout --detach master &&
	check_detached &&
	git status >out &&
	grep -i "detached" out &&
	git checkout master
'

test_done
