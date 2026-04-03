#!/bin/sh

test_description='git restore with various options'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	mkdir dir &&
	echo parent >dir/foo &&
	echo dummy >bar &&
	git add bar dir/foo &&
	git commit -m initial &&
	git tag initial &&
	test_tick &&
	echo head >dir/foo &&
	git add dir/foo &&
	git commit -m second
'

test_expect_success 'restore --staged restores index to HEAD' '
	echo modified >dir/foo &&
	git add dir/foo &&
	git restore --staged dir/foo &&
	git diff --cached --quiet HEAD
'

test_expect_success 'restore --source restores from specific commit' '
	git restore --source initial dir/foo &&
	echo parent >expect &&
	test_cmp expect dir/foo
'

test_expect_success 'restore without options restores worktree from index' '
	echo modified >bar &&
	git restore bar &&
	echo dummy >expect &&
	test_cmp expect bar
'

test_done
