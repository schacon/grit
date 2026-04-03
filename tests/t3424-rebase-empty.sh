#!/bin/sh
# Ported from git/t/t3424-rebase-empty.sh
# Tests for rebase of commits that start or become empty

test_description='git rebase of commits that start or become empty'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup test repository' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	test_write_lines 1 2 3 4 5 6 7 8 9 10 >numbers &&
	test_write_lines A B C D E F G H I J >letters &&
	git add numbers letters &&
	git commit -m A &&

	git branch upstream &&
	git branch localmods &&

	git checkout upstream &&
	test_write_lines A B C D E >letters &&
	git add letters &&
	git commit -m B &&

	test_write_lines 1 2 3 4 five 6 7 8 9 ten >numbers &&
	git add numbers &&
	git commit -m C &&

	git checkout localmods &&
	test_write_lines 1 2 3 4 five 6 7 8 9 10 >numbers &&
	git add numbers &&
	git commit -m C2
'

test_expect_success 'rebase does not leave state laying around' '
	git branch -f testing localmods &&
	git checkout testing &&
	git rebase upstream &&
	test_path_is_missing .git/CHERRY_PICK_HEAD &&
	test_path_is_missing .git/MERGE_MSG
'

test_expect_success 'rebase updates branch ref' '
	git checkout localmods &&
	git branch -f testing localmods &&
	git checkout testing &&
	old=$(git rev-parse HEAD) &&
	git rebase upstream &&
	new=$(git rev-parse HEAD) &&
	test "$old" != "$new"
'

test_expect_success 'rebased commit is ancestor of HEAD' '
	upstream_tip=$(git rev-parse upstream) &&
	git merge-base --is-ancestor $upstream_tip HEAD
'

test_done
