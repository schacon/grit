#!/bin/sh
# Ported from git/t/t5503-tagfollow.sh
# Simplified: tests tag following during fetch

test_description='test automatic tag following'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	test_tick &&
	echo ichi >file &&
	git add file &&
	git commit -m L &&

	git clone . cloned &&

	test_tick &&
	echo A >file &&
	git add file &&
	git commit -m A
'

test_expect_success 'fetch A (new commit)' '
	(
		cd cloned &&
		git fetch &&
		test "$(git rev-parse origin/main)" = "$(cd .. && git rev-parse main)"
	)
'

test_expect_success 'create tag on A' '
	git tag -a -m tag1 tag1 HEAD
'

test_expect_success 'fetch brings in tag' '
	(
		cd cloned &&
		git fetch --tags &&
		git show-ref tag1
	)
'

test_expect_success 'create more commits and tags' '
	test_tick &&
	echo B >file &&
	git add file &&
	git commit -m B &&
	git tag -a -m tag2 tag2 HEAD
'

test_expect_success 'fetch brings in new commits and tags' '
	(
		cd cloned &&
		git fetch --tags &&
		test "$(git rev-parse origin/main)" = "$(cd .. && git rev-parse main)" &&
		git show-ref tag2
	)
'

test_done
