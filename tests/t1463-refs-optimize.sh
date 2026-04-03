#!/bin/sh

test_description='git refs optimize should not change the branch semantic'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet support 'refs optimize' subcommand.
# We test pack-refs instead, which is the equivalent operation.

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file &&
	git add file &&
	git commit -m initial &&
	git branch branch1 &&
	git branch branch2 &&
	git tag v1.0
'

test_expect_success 'pack-refs preserves refs' '
	git show-ref >before &&
	git pack-refs --all &&
	git show-ref >after &&
	test_cmp before after
'

test_expect_success 'branches still resolve after pack-refs' '
	git rev-parse main >expect &&
	git rev-parse branch1 >actual &&
	test_cmp expect actual
'

test_expect_success 'tags still resolve after pack-refs' '
	git rev-parse main >expect &&
	git rev-parse v1.0 >actual &&
	test_cmp expect actual
'

test_expect_success 'refs optimize preserves refs' '
	git show-ref >before &&
	git refs optimize &&
	git show-ref >after &&
	test_cmp before after
'

test_done
