#!/bin/sh

test_description='checkout can handle basic index operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo content >file1 &&
	git add file1 &&
	git commit -m "add file1" &&
	echo updated >file1 &&
	git add file1 &&
	git commit -m "update file1"
'

test_expect_success '"reset <path>" updates the index' '
	git update-index --refresh &&
	git diff-files --quiet &&
	git diff-index --quiet --cached HEAD &&
	git reset HEAD^ file1 &&
	test_must_fail git diff-files --quiet &&
	git reset file1 &&
	git diff-files --quiet
'

test_expect_success 'checkout HEAD -- <path> restores file' '
	echo changed >file1 &&
	git checkout HEAD -- file1 &&
	echo updated >expect &&
	test_cmp expect file1
'

test_done
