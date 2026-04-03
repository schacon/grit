#!/bin/sh

test_description="config file in multi worktree"

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit start
'

test_expect_success 'config --worktree in single worktree' '
	git config --worktree foo.bar true &&
	test_cmp_config true foo.bar
'

test_expect_success 'add worktrees' '
	git worktree add wt1 &&
	git worktree add wt2
'

test_expect_success 'config in main worktree' '
	git config test.main value1 &&
	test_cmp_config value1 test.main
'

test_expect_success 'config in linked worktree is visible from main' '
	git -C wt1 config test.wt1 value2 &&
	git -C wt1 config --get test.wt1 >actual &&
	echo value2 >expect &&
	test_cmp expect actual
'

test_done
