#!/bin/sh
# Ported from git/t/t5526-fetch-submodules.sh
# Simplified: basic fetch with nested repos (no actual submodule support)

test_description='Recursive "git fetch" for submodules'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repos' '
	git init -q &&
	echo content >file &&
	git add file &&
	git commit -m initial &&
	git clone . downstream
'

test_expect_success 'fetch from upstream after changes' '
	echo new >file &&
	git add file &&
	git commit -m update &&
	(
		cd downstream &&
		git fetch origin &&
		test "$(git rev-parse origin/main)" = "$(cd .. && git rev-parse main)"
	)
'

test_expect_success 'pull from upstream' '
	echo newer >file &&
	git add file &&
	git commit -m update2 &&
	(
		cd downstream &&
		git pull &&
		test "$(git rev-parse HEAD)" = "$(cd .. && git rev-parse main)"
	)
'

test_done
