#!/bin/sh
# Ported from git/t/t5509-fetch-push-namespaces.sh
# Simplified: basic fetch/push with multiple branches

test_description='fetch/push with multiple branches'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	test_tick &&
	echo content >file &&
	git add file &&
	git commit -m initial &&
	git branch feature &&
	echo more >file &&
	git add file &&
	git commit -m second
'

test_expect_success 'clone gets all branches' '
	git clone . cloned &&
	(
		cd cloned &&
		git rev-parse origin/main &&
		git rev-parse origin/feature
	)
'

test_expect_success 'push new branch to clone' '
	git branch new-branch &&
	git clone . pusher &&
	(
		cd pusher &&
		git fetch origin &&
		git rev-parse origin/new-branch
	)
'

test_done
