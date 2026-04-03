#!/bin/sh
# Ported from git/t/t3908-stash-in-worktree.sh
# Basic stash tests

test_description='Test git stash basic operations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo initial >file &&
	git add file &&
	git commit -m "initial commit"
'

test_expect_success 'stash saves and restores changes' '
	echo modified >file &&
	git stash &&
	test "$(cat file)" = "initial" &&
	git stash pop &&
	test "$(cat file)" = "modified"
'

test_expect_success 'stash with staged changes' '
	echo staged >file &&
	git add file &&
	git stash &&
	test "$(cat file)" = "initial" &&
	git stash pop &&
	test "$(cat file)" = "staged"
'

test_expect_success 'stash list shows entries' '
	echo change1 >file &&
	git stash &&
	git stash list >actual &&
	test_line_count = 1 actual
'

test_expect_success 'multiple stashes stack' '
	echo change2 >file &&
	git stash &&
	git stash list >actual &&
	test_line_count = 2 actual
'

test_expect_success 'stash drop removes entry' '
	git stash drop &&
	git stash list >actual &&
	test_line_count = 1 actual
'

test_expect_success 'stash clear removes all entries' '
	git stash clear &&
	git stash list >actual &&
	test_must_be_empty actual
'

test_done
