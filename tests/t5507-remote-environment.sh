#!/bin/sh
# Ported from git/t/t5507-remote-environment.sh
# Simplified: basic remote push tests

test_description='check push to remote repository'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'set up push situation' '
	git init -q &&
	echo one >one.t &&
	git add one.t &&
	git commit -m one &&
	git tag one &&
	git init remote &&
	(cd remote && git config receive.denyCurrentBranch warn)
'

test_expect_success 'push to local remote' '
	git clone . clone &&
	(
		cd clone &&
		echo two >two.t &&
		git add two.t &&
		git commit -m two &&
		git push origin main
	)
'

test_done
