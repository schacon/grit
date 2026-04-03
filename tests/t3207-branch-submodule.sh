#!/bin/sh

test_description='git branch basic tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit A &&
	test_commit B &&
	test_commit C
'

test_expect_success 'create branch' '
	git branch feature &&
	git rev-parse feature >actual &&
	git rev-parse main >expect &&
	test_cmp expect actual
'

test_expect_success 'create branch at specific commit' '
	git branch old-feature A &&
	git rev-parse old-feature >actual &&
	git rev-parse A >expect &&
	test_cmp expect actual
'

test_expect_success 'list branches' '
	git branch >actual &&
	grep main actual &&
	grep feature actual &&
	grep old-feature actual
'

test_expect_success 'delete branch' '
	git branch -d old-feature &&
	test_must_fail git rev-parse old-feature
'

test_expect_success 'branch -D force deletes' '
	git branch to-delete &&
	git branch -D to-delete &&
	test_must_fail git rev-parse to-delete
'

test_expect_success 'checkout branch and commit' '
	git checkout feature &&
	test_commit D &&
	git rev-parse feature >actual &&
	git rev-parse HEAD >expect &&
	test_cmp expect actual
'

test_done
