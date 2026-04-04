#!/bin/sh

test_description='test worktree ref store api'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# The upstream test uses test-tool ref-store which is not available in
# grit. Instead, test worktree ref operations via normal git commands.

test_expect_success 'setup repo with worktree' '
	git init main-repo &&
	(
		cd main-repo &&
		git config init.defaultBranch main &&
		git checkout -b main &&
		git commit --allow-empty -m initial &&
		git checkout -b side &&
		git commit --allow-empty -m side &&
		git checkout main &&
		git worktree add ../wt side
	)
'

test_expect_success 'worktree has correct HEAD' '
	git -C wt rev-parse HEAD >actual &&
	git -C main-repo rev-parse side >expected &&
	test_cmp expected actual
'

test_expect_success 'worktree show-ref works' '
	git -C wt show-ref >actual &&
	grep refs/heads/main actual &&
	grep refs/heads/side actual
'

test_done
