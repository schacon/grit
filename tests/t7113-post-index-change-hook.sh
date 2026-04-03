#!/bin/sh
# Adapted from git/t/t7113-post-index-change-hook.sh
# Tests index operations that hooks monitor

test_description='index change operations'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init hook-repo &&
	cd hook-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	mkdir -p dir1 &&
	echo testing >dir1/file1.txt &&
	echo testing >dir1/file2.txt &&
	git add . &&
	git commit -m "initial"
'

test_expect_success 'add updates index' '
	cd hook-repo &&
	mkdir -p dir2 &&
	echo "new" >dir2/file1.txt &&
	git add dir2/file1.txt &&
	git status --porcelain >output &&
	test_grep "dir2/file1.txt" output
'

test_expect_success 'commit clears staged changes' '
	cd hook-repo &&
	git commit -m "second" &&
	git status --porcelain >output &&
	grep -v "^##" output | grep -v "^?" >filtered || true &&
	test_must_be_empty filtered
'

test_expect_success 'reset --soft does not change index' '
	cd hook-repo &&
	git tag here &&
	echo "change" >>dir1/file1.txt &&
	git add dir1/file1.txt &&
	git commit -m "third" &&
	git reset --soft here &&
	git diff --cached --name-only >changed &&
	test_grep "dir1/file1.txt" changed
'

test_done
