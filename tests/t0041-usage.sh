#!/bin/sh

test_description='Test commands behavior when given invalid argument value'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	test_commit "v1.0"
'

test_expect_success 'tag --contains <existent_tag>' '
	git tag --contains "v1.0" >actual 2>actual.err &&
	grep "v1.0" actual
'

test_expect_success 'branch --contains <existent_commit>' '
	git branch --contains "main" >actual 2>actual.err &&
	test_grep "main" actual
'

test_expect_success 'branch usage error' '
	test_must_fail git branch --noopt >actual 2>actual.err
'

test_expect_success 'for-each-ref --contains <existent_object>' '
	git for-each-ref --contains "main" >actual 2>actual.err
'

test_done
