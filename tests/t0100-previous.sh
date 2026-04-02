#!/bin/sh
# Tests for checkout -, switch -, and previous branch tracking.

test_description='grit checkout - and switch - previous branch tracking'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ===========================================================================
# Setup
# ===========================================================================

test_expect_success 'setup: create repo with multiple branches' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo initial >file &&
	git add file &&
	git commit -m "initial commit" &&
	git branch branchA &&
	git branch branchB &&
	git branch branchC &&
	git branch branchD
'

# ===========================================================================
# Basic checkout - functionality
# ===========================================================================

test_expect_success '"checkout -" fails when no previous branch exists' '
	cd repo &&
	test_must_fail git checkout -
'

test_expect_success '"checkout -" switches back after single branch switch' '
	cd repo &&
	git checkout branchA &&
	git checkout master &&
	git checkout - &&
	echo refs/heads/branchA >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" toggles between two branches' '
	cd repo &&
	git checkout master &&
	git checkout branchA &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual &&
	git checkout - &&
	echo refs/heads/branchA >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" after multiple switches goes to last branch' '
	cd repo &&
	git checkout master &&
	git checkout branchA &&
	git checkout branchB &&
	git checkout - &&
	echo refs/heads/branchA >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" after switching through many branches' '
	cd repo &&
	git checkout master &&
	git checkout branchA &&
	git checkout branchB &&
	git checkout branchC &&
	git checkout - &&
	echo refs/heads/branchB >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" after four branch switches' '
	cd repo &&
	git checkout master &&
	git checkout branchA &&
	git checkout branchB &&
	git checkout branchC &&
	git checkout branchD &&
	git checkout - &&
	echo refs/heads/branchC >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ===========================================================================
# checkout - with merge/commit operations in between
# ===========================================================================

test_expect_success '"checkout -" still works after commits on current branch' '
	cd repo &&
	git checkout master &&
	git checkout branchA &&
	echo extra >extra-file &&
	git add extra-file &&
	git commit -m "extra commit on branchA" &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" preserves previous after checkout with path' '
	cd repo &&
	git checkout master &&
	git checkout branchB &&
	git checkout master -- file &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ===========================================================================
# checkout - with branch creation
# ===========================================================================

test_expect_success '"checkout -" works after creating and switching to new branch' '
	cd repo &&
	git checkout master &&
	git checkout -b newbranch1 &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -b" then "checkout -" goes back to source' '
	cd repo &&
	git checkout branchA &&
	git checkout -b newbranch2 &&
	git checkout - &&
	echo refs/heads/branchA >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -b" from branchB then "checkout -" returns to branchB' '
	cd repo &&
	git checkout branchB &&
	git checkout -b newbranch3 &&
	git checkout - &&
	echo refs/heads/branchB >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ===========================================================================
# switch - (same as checkout -)
# ===========================================================================

test_expect_success '"switch -" toggles like "checkout -"' '
	cd repo &&
	git checkout master &&
	git switch branchA &&
	git switch - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"switch -" after multiple switches' '
	cd repo &&
	git switch master &&
	git switch branchA &&
	git switch branchB &&
	git switch - &&
	echo refs/heads/branchA >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"switch -" toggles back and forth' '
	cd repo &&
	git switch master &&
	git switch branchC &&
	git switch - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual &&
	git switch - &&
	echo refs/heads/branchC >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'mixed checkout and switch - track the same previous' '
	cd repo &&
	git checkout master &&
	git switch branchA &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"switch -c newbranch" then "switch -" returns to previous' '
	cd repo &&
	git switch master &&
	git switch -c newbranch4 &&
	git switch - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ===========================================================================
# Detached HEAD interactions
# ===========================================================================

test_expect_success '"checkout -" after detached HEAD returns to named branch' '
	cd repo &&
	git checkout master &&
	head_oid=$(git rev-parse HEAD) &&
	git checkout "$head_oid" &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" from named branch to detached and back' '
	cd repo &&
	git checkout branchA &&
	head_oid=$(git rev-parse HEAD) &&
	git checkout master &&
	git checkout "$head_oid" &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"switch -" after detached HEAD via checkout' '
	cd repo &&
	git checkout branchB &&
	head_oid=$(git rev-parse HEAD) &&
	git checkout "$head_oid" &&
	git switch - &&
	echo refs/heads/branchB >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

# ===========================================================================
# Edge cases
# ===========================================================================

test_expect_success '"checkout -" repeated three times cycles correctly' '
	cd repo &&
	git checkout master &&
	git checkout branchA &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual &&
	git checkout - &&
	echo refs/heads/branchA >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" in fresh repo with only one commit' '
	git init fresh-repo &&
	cd fresh-repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x &&
	git add x &&
	git commit -m "first" &&
	test_must_fail git checkout -
'

test_expect_success '"switch -" in fresh repo fails' '
	git init fresh-repo2 &&
	cd fresh-repo2 &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo x >x &&
	git add x &&
	git commit -m "first" &&
	test_must_fail git switch -
'

test_expect_success '"checkout -" works after fast-forward merge' '
	cd repo &&
	git checkout master &&
	git checkout branchA &&
	git checkout master &&
	git checkout - &&
	echo refs/heads/branchA >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" works after reset' '
	cd repo &&
	git checkout master &&
	git checkout branchA &&
	git reset HEAD~1 &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '"checkout -" preserves working tree changes' '
	cd repo &&
	git checkout master &&
	git checkout branchB &&
	echo "dirty" >untracked-file &&
	git checkout - &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_done
