#!/bin/sh

test_description='update-index refresh tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo content >file &&
	echo content >other &&
	git add file other &&
	git commit -m "initial import"
'

test_expect_success 'update-index --refresh with clean worktree' '
	git update-index --refresh &&
	git diff-files --quiet
'

test_expect_success 'update-index --refresh detects changes' '
	echo modified >file &&
	test_must_fail git diff-files --quiet &&
	echo content >file &&
	git update-index --refresh &&
	git diff-files --quiet
'

test_expect_success 'update-index --really-refresh forces restat' '
	git update-index --really-refresh &&
	git diff-files --quiet
'

test_done
