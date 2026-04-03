#!/bin/sh

test_description='Test pruning of repositories'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_tick && git commit --allow-empty -m main &&
	test_tick && git commit --allow-empty -m second &&
	test_tick && git commit --allow-empty -m third
'

test_expect_success 'rev-list shows all commits' '
	git rev-list HEAD >actual &&
	test_line_count = 3 actual
'

test_expect_success 'create dangling object' '
	blob=$(echo "unreachable" | git hash-object -w --stdin) &&
	git cat-file -e $blob
'

test_expect_success 'prune removes unreachable loose objects' '
	git prune --expire=now &&
	test_must_fail git cat-file -e $blob
'

test_expect_success 'prune keeps reachable objects' '
	git rev-list HEAD >actual &&
	test_line_count = 3 actual
'

test_done
