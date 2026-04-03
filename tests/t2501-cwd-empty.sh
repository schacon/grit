#!/bin/sh

test_description='Test handling of the current working directory becoming empty'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	test_commit init &&

	mkdir -p foo/bar &&
	echo content >foo/bar/baz.t &&
	git add foo/bar/baz.t &&
	git commit -m "add foo/bar/baz" &&
	git tag with-foo
'

test_expect_success 'checkout removes directory when switching branches' '
	git branch no-foo init &&
	git checkout no-foo &&
	test_path_is_missing foo &&
	git checkout with-foo &&
	test_path_is_file foo/bar/baz.t
'

test_expect_success 'reset --hard cleans up worktree' '
	git checkout with-foo &&
	echo extra >foo/bar/extra &&
	git add foo/bar/extra &&
	git reset --hard with-foo &&
	test_path_is_missing foo/bar/extra
'

test_done
