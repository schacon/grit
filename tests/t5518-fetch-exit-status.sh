#!/bin/sh
# Ported from git/t/t5518-fetch-exit-status.sh

test_description='fetch exit status test'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	>file &&
	git add file &&
	git commit -m initial &&

	git checkout -b side &&
	echo side >file &&
	git commit -a -m side &&

	git checkout main &&
	echo next >file &&
	git commit -a -m next
'

test_expect_success 'non-fast-forward fetch fails' '
	test_must_fail git fetch . main:side
'

test_done
