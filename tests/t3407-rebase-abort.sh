#!/bin/sh

test_description='git rebase --abort tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	echo a >a &&
	git add a &&
	test_tick &&
	git commit -m a &&
	git branch to-rebase &&

	echo b >a &&
	git add a &&
	test_tick &&
	git commit -m b &&

	echo c >a &&
	git add a &&
	test_tick &&
	git commit -m c &&

	git checkout to-rebase &&
	echo d >a &&
	git add a &&
	test_tick &&
	git commit -m d
'

test_expect_success 'rebase --abort after conflict' '
	pre_rebase=$(git rev-parse HEAD) &&
	test_must_fail git rebase main &&
	git rebase --abort &&
	test "$(git rev-parse HEAD)" = "$pre_rebase" &&
	test "$(git symbolic-ref HEAD)" = refs/heads/to-rebase
'

test_expect_success 'rebase --abort restores working tree' '
	test_must_fail git rebase main &&
	git rebase --abort &&
	echo d >expect &&
	test_cmp expect a
'

test_done
