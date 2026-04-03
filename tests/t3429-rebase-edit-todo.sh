#!/bin/sh
# Ported from git/t/t3429-rebase-edit-todo.sh
# Tests for basic rebase functionality

test_description='git rebase basic operations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo base >file &&
	git add file &&
	git commit -m "A" &&
	git tag A &&
	echo B >file &&
	git add file &&
	git commit -m "B" &&
	git tag B &&
	echo C >file &&
	git add file &&
	git commit -m "C" &&
	git tag C &&
	git checkout -b side A &&
	echo D >other &&
	git add other &&
	git commit -m "D" &&
	git tag D &&
	echo E >other2 &&
	git add other2 &&
	git commit -m "E" &&
	git tag E
'

test_expect_success 'rebase side onto master' '
	git checkout side &&
	git rebase master &&
	git log --format=%s -n4 >actual &&
	test_write_lines E D C B >expect &&
	test_cmp expect actual
'

test_expect_success 'rebase is no-op when already up to date' '
	git checkout master &&
	oldhead=$(git rev-parse HEAD) &&
	git rebase master &&
	newhead=$(git rev-parse HEAD) &&
	test "$oldhead" = "$newhead"
'

test_expect_success 'rebase --abort works during conflict' '
	git checkout -b conflict-test A &&
	echo conflict >file &&
	git add file &&
	git commit -m "conflict with B" &&
	old=$(git rev-parse HEAD) &&
	test_must_fail git rebase master &&
	git rebase --abort &&
	new=$(git rev-parse HEAD) &&
	test "$old" = "$new"
'

test_done
