#!/bin/sh

test_description='Test merge with directory/file conflicts'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'prepare repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	echo Hello >init &&
	git add init &&
	git commit -m initial &&

	git branch B &&
	mkdir dir &&
	echo foo >dir/foo &&
	git add dir/foo &&
	git commit -m "File: dir/foo" &&

	git checkout B &&
	echo file dir >dir &&
	git add dir &&
	git commit -m "File: dir"
'

test_expect_success 'Merge with d/f conflicts' '
	cd repo &&
	test_must_fail git merge -m "merge msg" main
'

test_expect_success 'reset after d/f conflict' '
	cd repo &&
	git reset --hard &&
	git checkout main
'

test_expect_success 'Simple merge in repo with interesting pathnames' '
	cd repo &&
	git init name-ordering &&
	(
		cd name-ordering &&
		git config user.name "Test" &&
		git config user.email "test@test" &&

		mkdir -p foo/bar &&
		mkdir -p foo/bar-2 &&
		>foo/bar/baz &&
		>foo/bar-2/baz &&
		git add . &&
		git commit -m initial &&

		git branch topic &&
		git branch other &&

		git checkout other &&
		echo other >foo/bar-2/baz &&
		git add -u &&
		git commit -m other &&

		git checkout topic &&
		echo topic >foo/bar/baz &&
		git add -u &&
		git commit -m topic &&

		git merge other &&
		git ls-files -s >out &&
		test_line_count = 2 out
	)
'

test_done
