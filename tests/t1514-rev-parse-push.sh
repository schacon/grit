#!/bin/sh
# Ported from upstream t1514-rev-parse-push.sh
# Tests <branch>@{push} syntax.
# grit supports @{push} once remote-tracking refs are fetched.

test_description='test <branch>@{push} syntax'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init --bare parent.git &&
	git init --bare other.git &&
	git init repo &&
	(
		cd repo &&
		git config user.name "Test" &&
		git config user.email "test@test.com" &&
		git remote add origin "$TRASH_DIRECTORY/parent.git" &&
		git remote add other "$TRASH_DIRECTORY/other.git" &&
		echo content >file &&
		git add file &&
		test_tick &&
		git commit -m base &&
		git push origin main &&
		git fetch origin &&
		git branch --set-upstream-to=origin/main main &&
		git checkout -b topic &&
		git push origin topic &&
		git push other topic &&
		git fetch origin &&
		git fetch other &&
		git branch --set-upstream-to=origin/topic topic
	)
'

test_expect_success 'push creates remote refs' '
	(
		cd parent.git &&
		git rev-parse refs/heads/main &&
		git rev-parse refs/heads/topic
	)
'

test_expect_success 'upstream tracking is configured' '
	(
		cd repo &&
		test "$(git config branch.main.remote)" = "origin" &&
		test "$(git config branch.main.merge)" = "refs/heads/main"
	)
'

test_expect_success '@{push} resolves with default=simple on main' '
	(
		cd repo &&
		git checkout main &&
		git config push.default simple &&
		git rev-parse main@{push} >actual &&
		git rev-parse origin/main >expect &&
		test_cmp expect actual
	)
'

test_expect_success '@{push} with default=current on topic' '
	(
		cd repo &&
		git checkout topic &&
		git config push.default current &&
		git rev-parse topic@{push} >actual &&
		git rev-parse origin/topic >expect &&
		test_cmp expect actual
	)
'

test_expect_success '@{push} with pushremote defined' '
	(
		cd repo &&
		git checkout topic &&
		git config push.default current &&
		git config branch.topic.pushremote other &&
		git rev-parse topic@{push} >actual &&
		git rev-parse other/topic >expect &&
		test_cmp expect actual
	)
'

test_done
