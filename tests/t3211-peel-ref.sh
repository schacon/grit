#!/bin/sh

test_description='tests for the peel_ref optimization of packed-refs'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'create annotated tag in refs/tags' '
	test_commit base &&
	git tag -m annotated foo
'

test_expect_success 'create annotated tag outside of refs/tags' '
	git update-ref refs/outside/foo refs/tags/foo
'

# This matches show-ref's output
print_ref() {
	echo "$(git rev-parse "$1") $1"
}

test_expect_success 'set up expected show-ref output' '
	{
		print_ref "refs/heads/main" &&
		print_ref "refs/outside/foo" &&
		print_ref "refs/outside/foo^{}" &&
		print_ref "refs/tags/base" &&
		print_ref "refs/tags/foo" &&
		print_ref "refs/tags/foo^{}"
	} >expect
'

test_expect_success 'refs are peeled outside of refs/tags (loose)' '
	git show-ref -d >actual &&
	test_cmp expect actual
'

test_done
