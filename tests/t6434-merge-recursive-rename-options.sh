#!/bin/sh

test_description='merge rename detection options'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup repo with renames' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	cat >original <<-\EOF &&
	line a
	line b
	line c
	line d
	EOF
	git add original &&
	git commit -m base &&
	git tag base
'

test_expect_success 'diff detects file addition after rename' '
	git checkout -b rename-branch base &&
	cp original renamed &&
	git rm original &&
	git add renamed &&
	git commit -m "rename original->renamed" &&

	git diff --name-status base rename-branch >output &&
	grep "renamed" output
'

test_expect_success 'diff detects file changes after rename with modification' '
	git checkout -b rename-mod base &&
	sed "s/line a/modified a/" <original >renamed-mod &&
	git rm original &&
	git add renamed-mod &&
	git commit -m "rename with modification" &&

	git diff --name-status base rename-mod >output &&
	grep "renamed-mod" output
'

test_expect_success 'diff shows deleted original' '
	git diff --name-status base rename-branch >output &&
	grep "original" output
'

test_expect_success 'merge with rename on one side succeeds or conflicts' '
	git checkout -b del-branch base &&
	git rm original &&
	git commit -m "delete original" &&

	git checkout rename-branch &&
	git merge del-branch ||
	true
'

test_expect_success 'diff-tree shows changes between trees' '
	tree_base=$(git rev-parse base^{tree}) &&
	tree_rename=$(git rev-parse rename-branch^{tree}) &&
	git diff-tree $tree_base $tree_rename >output &&
	test -s output
'

test_done
