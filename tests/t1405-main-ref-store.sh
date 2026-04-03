#!/bin/sh

test_description='test main ref store api basics'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	test_commit one
'

test_expect_success 'symbolic-ref HEAD points to main' '
	echo refs/heads/main >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'for-each-ref lists refs' '
	git for-each-ref refs/heads/ >actual &&
	grep main actual
'

test_expect_success 'for-each-ref is sorted' '
	git for-each-ref refs/heads/ >actual &&
	sort actual >expected &&
	test_cmp expected actual
'

test_expect_success 'show-ref lists refs' '
	git show-ref >actual &&
	grep refs/heads/main actual &&
	grep refs/tags/one actual
'

test_expect_success 'update-ref creates and deletes refs' '
	COMMIT_OID=$(git rev-parse HEAD) &&
	git update-ref refs/heads/test-branch $COMMIT_OID &&
	git rev-parse refs/heads/test-branch >actual &&
	echo $COMMIT_OID >expected &&
	test_cmp expected actual &&
	git update-ref -d refs/heads/test-branch &&
	test_must_fail git rev-parse --verify refs/heads/test-branch
'

test_done
