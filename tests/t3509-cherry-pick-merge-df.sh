#!/bin/sh
# Ported from git/t/t3509-cherry-pick-merge-df.sh
# Test cherry-pick with directory/file scenarios

test_description='Test cherry-pick with directory/file scenarios'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	mkdir a &&
	>a/f &&
	git add a &&
	git commit -m a &&
	git tag initial
'

test_expect_success 'cherry-pick commit that adds a file' '
	git checkout -b add-file initial &&
	echo content >newfile &&
	git add newfile &&
	git commit -m "add newfile" &&
	git tag add-file-tag &&

	git checkout main &&
	git cherry-pick add-file-tag &&
	test_path_is_file newfile &&
	test "$(cat newfile)" = "content"
'

test_expect_success 'cherry-pick commit that modifies a file' '
	git checkout -b modify-file initial &&
	echo modified >a/f &&
	git add a/f &&
	git commit -m "modify a/f" &&
	git tag modify-tag &&

	git checkout main &&
	git reset --hard initial &&
	git cherry-pick modify-tag &&
	test "$(cat a/f)" = "modified"
'

test_expect_success 'cherry-pick commit that removes a file' '
	git checkout main &&
	git reset --hard initial &&
	echo extra >extra &&
	git add extra &&
	git commit -m "add extra" &&

	git checkout -b remove-file HEAD &&
	git rm extra &&
	git commit -m "remove extra" &&
	git tag remove-tag &&

	git checkout main &&
	git cherry-pick remove-tag &&
	test_path_is_missing extra
'

test_done
