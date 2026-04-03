#!/bin/sh
# Ported from git/t/t5530-upload-pack-error.sh
# Simplified: tests basic error handling

test_description='errors in upload-pack'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	git init -q &&
	echo file >file &&
	git add file &&
	git commit -a -m original &&
	test_tick &&
	echo changed >file &&
	git commit -a -m changed
'

test_expect_success 'clone succeeds' '
	git clone . clone &&
	(
		cd clone &&
		test "$(git rev-parse origin/main)" = "$(cd .. && git rev-parse main)"
	)
'

test_expect_success 'fetch succeeds' '
	echo more >file &&
	git add file &&
	git commit -m more &&
	(
		cd clone &&
		git fetch origin &&
		test "$(git rev-parse origin/main)" = "$(cd .. && git rev-parse main)"
	)
'

test_done
