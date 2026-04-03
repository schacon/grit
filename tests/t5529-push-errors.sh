#!/bin/sh
# Ported from git/t/t5529-push-errors.sh

test_description='detect some push errors early (before contacting remote)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup commits' '
	git init -q &&
	echo one >one.t &&
	git add one.t &&
	git commit -m one &&
	git tag one
'

test_expect_success 'setup remote' '
	git init --bare remote.git &&
	git remote add origin remote.git
'

test_expect_success 'detect missing branches early' '
	test_must_fail git push origin missing
'

test_done
