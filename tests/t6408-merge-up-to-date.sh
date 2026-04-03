#!/bin/sh

test_description='merge fast-forward and up to date'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	>file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag c0 &&

	echo second >file &&
	git add file &&
	test_tick &&
	git commit -m second &&
	git tag c1 &&
	git branch test &&
	echo third >file &&
	git add file &&
	test_tick &&
	git commit -m third &&
	git tag c2
'

test_expect_success 'merge up-to-date (already contains merged commit)' '
	cd repo &&
	git reset --hard c1 &&
	test_tick &&
	git merge c0 &&
	expect=$(git rev-parse c1) &&
	current=$(git rev-parse HEAD) &&
	test "$expect" = "$current"
'

test_expect_success 'merge fast-forward' '
	cd repo &&
	git reset --hard c0 &&
	test_tick &&
	git merge c1 &&
	expect=$(git rev-parse c1) &&
	current=$(git rev-parse HEAD) &&
	test "$expect" = "$current"
'

test_expect_success 'merge already up-to-date message' '
	cd repo &&
	git reset --hard c2 &&
	test_tick &&
	git merge c1 2>err &&
	expect=$(git rev-parse c2) &&
	current=$(git rev-parse HEAD) &&
	test "$expect" = "$current"
'

test_done
