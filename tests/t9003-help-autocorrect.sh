#!/bin/sh
# Ported from git/t/t9003-help-autocorrect.sh
# Tests for help.autocorrect configuration

test_description='help.autocorrect finding a match'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git commit --allow-empty -m "a single log entry"
'

test_expect_success 'typo shows similar command' '
	cd repo &&
	test_must_fail git stauts 2>actual &&
	grep "status" actual
'

test_expect_success 'autocorrect=0 shows candidates' '
	cd repo &&
	git config help.autocorrect 0 &&
	test_must_fail git stauts 2>actual &&
	grep "status" actual
'

test_expect_success 'autocorrect=immediate runs command' '
	cd repo &&
	git config help.autocorrect immediate &&
	git stauts >actual 2>&1
'

test_expect_success 'autocorrect=-1 runs command immediately' '
	cd repo &&
	git config help.autocorrect -1 &&
	git stauts >actual 2>&1
'

test_expect_success 'autocorrect=never declines altogether' '
	cd repo &&
	git config help.autocorrect never &&
	test_must_fail git stauts 2>actual &&
	grep "is not a.*command" actual
'

test_done
