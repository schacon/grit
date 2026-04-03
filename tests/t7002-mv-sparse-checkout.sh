#!/bin/sh

test_description='git mv with sparse checkout'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	mkdir sub &&
	echo a >a &&
	echo b >b &&
	echo sub_a >sub/a &&
	echo sub_b >sub/b &&
	git add . &&
	git commit -m initial
'

test_expect_success 'git mv basic rename' '
	git mv a c &&
	git status --porcelain >actual &&
	grep "^R\|renamed" actual || grep "c" actual &&
	git commit -m "rename a to c" &&
	test_path_is_file c &&
	test_path_is_missing a
'

test_expect_success 'git mv into directory' '
	git mv c sub/ &&
	git commit -m "move c into sub" &&
	test_path_is_file sub/c &&
	test_path_is_missing c
'

test_expect_success 'git mv with force overwrites' '
	echo conflict >sub/b &&
	echo new >replace &&
	git add replace sub/b &&
	git commit -m "setup for force" &&
	git mv -f replace sub/b &&
	git commit -m "force move" &&
	echo new >expect &&
	test_cmp expect sub/b &&
	test_path_is_missing replace
'

test_expect_success 'git mv fails for nonexistent source' '
	test_must_fail git mv nonexistent somewhere 2>err &&
	test -s err
'

test_expect_success 'git mv dry-run does not change files' '
	echo before >dryrun &&
	git add dryrun &&
	git commit -m "add dryrun" &&
	git mv -n dryrun dryrun-moved 2>err &&
	test_path_is_file dryrun &&
	test_path_is_missing dryrun-moved
'

test_done
