#!/bin/sh
# Ported from git/t/t5512-ls-remote.sh
# Tests for 'git ls-remote'

test_description='git ls-remote'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	>file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag mark &&
	git tag mark1.1 &&
	git tag mark1.2 &&
	git tag mark1.10
'

test_expect_success 'ls-remote .git shows refs' '
	git ls-remote .git >actual &&
	test_grep "refs/heads/main" actual &&
	test_grep "refs/tags/mark" actual
'

test_expect_success 'ls-remote --tags .git' '
	git ls-remote --tags .git >actual &&
	test_grep "refs/tags/mark" actual &&
	! test_grep "refs/heads/" actual
'

test_expect_success 'ls-remote --heads .git' '
	git ls-remote --heads .git >actual &&
	test_grep "refs/heads/main" actual &&
	! test_grep "refs/tags/" actual
'

test_expect_success 'ls-remote --refs .git excludes pseudo-refs' '
	git ls-remote --refs .git >actual &&
	! test_grep "HEAD" actual
'

test_expect_success 'ls-remote --symref .git' '
	git ls-remote --symref .git >actual &&
	test_grep "ref: refs/heads/main" actual
'

test_expect_success 'ls-remote --quiet .git produces no output' '
	git ls-remote --quiet .git >actual &&
	test_must_be_empty actual
'

test_done
