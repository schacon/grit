#!/bin/sh

test_description='git rebase basic tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	echo hello >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	echo goodbye >file &&
	git add file &&
	test_tick &&
	git commit -m second &&
	git checkout -b side HEAD^ &&
	echo world >git &&
	git add git &&
	test_tick &&
	git commit -m side &&
	git checkout main &&
	git branch test side
'

test_expect_success 'rebase applies commits' '
	git checkout test &&
	git reset --hard side &&
	git rebase main &&
	test "z$(cat git)" = zworld
'

test_expect_success 'rebase preserves file content from main' '
	test "z$(cat file)" = zgoodbye
'

test_expect_success 'rebased commit is on top of main' '
	git rev-parse main >expect &&
	git rev-parse HEAD^ >actual &&
	test_cmp expect actual
'

test_done
