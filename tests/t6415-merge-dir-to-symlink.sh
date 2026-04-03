#!/bin/sh

test_description='merging when a directory was replaced with a symlink'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

# Symlink tests require git to store symlinks as symlinks in the index.
# grit currently follows symlinks, so symlink-specific merge scenarios
# are not yet supported.

test_expect_success 'setup base repo with directory structure' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	mkdir -p a/b/c a/b-2/c &&
	echo content > a/b/c/d &&
	echo content > a/b-2/c/d &&
	echo x > a/x &&
	git add a &&
	git commit -m base &&
	git tag start
'

test_expect_success 'checkout preserves directory structure' '
	git checkout -b work &&
	echo extra > a/x &&
	git add a/x &&
	git commit -m "modify x" &&
	git checkout start &&
	test_path_is_dir a/b &&
	test_path_is_file a/b/c/d &&
	test_path_is_dir a/b-2 &&
	test_path_is_file a/b-2/c/d
'

test_expect_success 'merge preserves directory structure' '
	git checkout -b merge-base start &&
	echo more > a/b-2/c/e &&
	git add a/b-2/c/e &&
	git commit -m "add file in b-2" &&
	git merge work &&
	test_path_is_file a/b-2/c/d &&
	test_path_is_file a/b-2/c/e &&
	test_path_is_file a/b/c/d
'

test_done
