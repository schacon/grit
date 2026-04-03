#!/bin/sh
# Ported from git/t/t5505-remote.sh
# Tests for 'git remote' porcelain-ish commands

test_description='git remote porcelain-ish'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

setup_repository () {
	mkdir "$1" && (
	cd "$1" &&
	git init -q &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	>file &&
	git add file &&
	test_tick &&
	git commit -m "Initial" &&
	git checkout -b side &&
	>elif &&
	git add elif &&
	test_tick &&
	git commit -m "Second" &&
	git checkout main
	)
}

test_expect_success 'setup' '
	setup_repository one &&
	setup_repository two &&
	(
		cd two &&
		git branch another
	) &&
	git clone one test
'

test_expect_success 'remote information for the origin' '
	(
		cd test &&
		test "$(git remote)" = "origin"
	)
'

test_expect_success 'add another remote' '
	(
		cd test &&
		git remote add second ../two &&
		git remote | sort >actual &&
		printf "origin\nsecond\n" >expect &&
		test_cmp expect actual
	)
'

test_expect_success 'remote -v shows URLs' '
	(
		cd test &&
		git remote -v >output &&
		grep "origin" output &&
		grep "second" output
	)
'

test_expect_success 'remote get-url' '
	(
		cd test &&
		git remote get-url origin >actual &&
		test_grep "one" actual
	)
'

test_expect_success 'remote set-url' '
	(
		cd test &&
		git remote set-url second /tmp/newpath &&
		git remote get-url second >actual &&
		echo "/tmp/newpath" >expect &&
		test_cmp expect actual &&
		git remote set-url second ../two
	)
'

test_expect_success 'remote rename' '
	(
		cd test &&
		git remote rename second renamed &&
		git remote >actual &&
		grep renamed actual &&
		! grep second actual &&
		git remote rename renamed second
	)
'

test_expect_success 'remove remote' '
	(
		cd test &&
		git remote add removeme ../one &&
		git remote remove removeme &&
		git remote >actual &&
		! grep removeme actual
	)
'

test_expect_success 'remove errors on non-existent remote' '
	(
		cd test &&
		test_must_fail git remote rm nonexistent 2>err &&
		test_grep "No such remote" err
	)
'

test_expect_success 'remote show' '
	(
		cd test &&
		git remote show origin >output &&
		test_grep "origin" output
	)
'

test_expect_success 'fetch from remote' '
	(
		cd test &&
		git fetch second
	)
'

test_expect_success 'remote rm (alias)' '
	(
		cd test &&
		git remote add temp ../one &&
		git remote rm temp &&
		git remote >actual &&
		! grep temp actual
	)
'

test_done
