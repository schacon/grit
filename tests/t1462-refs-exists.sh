#!/bin/sh
# Ported from upstream t1462-refs-exists.sh
# Original uses 'git refs exists' which grit does not have.
# We test using 'git show-ref --exists' which has equivalent behavior.

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	tree=$(git write-tree) &&
	commit=$(echo base | git commit-tree "$tree") &&
	git update-ref refs/heads/master "$commit" &&
	git update-ref refs/heads/main "$commit" &&
	git update-ref refs/heads/side "$commit"
'

test_expect_success '--exists with existing reference' '
	git show-ref --exists refs/heads/side
'

test_expect_success '--exists with missing reference' '
	test_must_fail git show-ref --exists refs/heads/does-not-exist
'

test_expect_success '--exists does not use DWIM' '
	test_must_fail git show-ref --exists side 2>err &&
	grep "reference does not exist" err
'

test_expect_success '--exists with HEAD' '
	git show-ref --exists HEAD
'

test_expect_success '--exists with arbitrary symref' '
	git symbolic-ref refs/symref refs/heads/side &&
	git show-ref --exists refs/symref
'

test_expect_success '--exists with directory reports missing ref' '
	test_must_fail git show-ref --exists refs/heads 2>err &&
	grep "reference does not exist" err
'

test_expect_success '--exists succeeds for refs/heads/master' '
	git show-ref --exists refs/heads/master
'

test_expect_success '--exists fails for nonexistent tag ref' '
	test_must_fail git show-ref --exists refs/tags/nonexistent 2>err &&
	grep "reference does not exist" err
'

test_done
