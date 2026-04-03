#!/bin/sh

test_description='test main ref store api'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not have test-tool ref-store. We test the public API via
# regular git commands instead.

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file &&
	git add file &&
	git commit -m one &&
	git rev-parse HEAD >.head_oid
'

test_expect_success 'create symbolic ref' '
	git symbolic-ref FOO refs/heads/main &&
	echo refs/heads/main >expected &&
	git symbolic-ref FOO >actual &&
	test_cmp expected actual
'

test_expect_success 'delete ref via update-ref -d' '
	git tag -a -m new-tag new-tag HEAD &&
	git rev-parse refs/tags/new-tag -- &&
	git update-ref -d refs/tags/new-tag &&
	test_must_fail git rev-parse refs/tags/new-tag --
'

test_expect_success 'for-each-ref lists refs' '
	git for-each-ref --format="%(refname)" refs/heads/ >actual &&
	grep main actual
'

test_expect_success 'for-each-ref is sorted' '
	git branch zebra &&
	git branch alpha &&
	git for-each-ref --format="%(refname)" refs/heads/ >actual &&
	sort actual >expected &&
	test_cmp expected actual
'

test_expect_success 'symbolic-ref HEAD resolves' '
	echo refs/heads/main >expected &&
	git symbolic-ref HEAD >actual &&
	test_cmp expected actual
'

test_expect_success 'update-ref with old value verification' '
	git checkout -b test-branch &&
	echo change >file &&
	git add file &&
	git commit -m "change" &&
	OLD=$(git rev-parse test-branch) &&
	echo newer >file &&
	git add file &&
	git commit -m "newer" &&
	NEW=$(git rev-parse test-branch) &&
	git update-ref refs/heads/test-branch "$OLD" "$NEW" &&
	echo "$OLD" >expected &&
	git rev-parse refs/heads/test-branch >actual &&
	test_cmp expected actual
'

test_expect_success 'delete ref with old value' '
	OID=$(git rev-parse test-branch) &&
	git checkout main &&
	git update-ref -d refs/heads/test-branch "$OID" &&
	test_must_fail git rev-parse refs/heads/test-branch --
'

test_done
