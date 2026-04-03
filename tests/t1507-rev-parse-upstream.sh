#!/bin/sh

test_description='test <branch>@{upstream} syntax'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit supports @{upstream} for the current branch but may not support
# branch@{u} for other branches or --symbolic-full-name.

test_expect_success 'setup' '
	git init upstream-repo &&
	(
		cd upstream-repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo 1 >file &&
		git add file &&
		git commit -m "1" &&
		git checkout -b side &&
		echo 2 >file &&
		git add file &&
		git commit -m "2" &&
		git checkout main
	) &&
	git clone upstream-repo clone &&
	(
		cd clone &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		echo 4 >file &&
		git add file &&
		git commit -m "4" &&
		git branch --track my-side origin/side
	)
'

test_expect_success 'clone creates tracking branches' '
	(
		cd clone &&
		git branch -r >actual &&
		grep "origin/main" actual
	)
'

test_expect_success '@{upstream} resolves to remote tracking branch' '
	(
		cd clone &&
		git rev-parse @{upstream} >actual &&
		git rev-parse origin/main >expect &&
		test_cmp expect actual
	)
'

test_expect_success '@{u} shorthand resolves to upstream' '
	(
		cd clone &&
		git rev-parse @{u} >actual &&
		git rev-parse origin/main >expect &&
		test_cmp expect actual
	)
'

test_expect_failure 'other-branch@{upstream} resolves' '
	(
		cd clone &&
		git rev-parse my-side@{upstream} >actual &&
		git rev-parse origin/side >expect &&
		test_cmp expect actual
	)
'

test_expect_success 'rev-parse --symbolic-full-name @{upstream}' '
	(
		cd clone &&
		echo refs/remotes/origin/main >expect &&
		git rev-parse --symbolic-full-name @{upstream} >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'not-tracking@{upstream} fails' '
	(
		cd clone &&
		test_must_fail git rev-parse non-tracking@{upstream}
	)
'

test_expect_success 'branch tracking is configured correctly' '
	(
		cd clone &&
		test "$(git config branch.main.remote)" = "origin" &&
		test "$(git config branch.main.merge)" = "refs/heads/main"
	)
'

test_done
