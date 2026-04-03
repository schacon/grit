#!/bin/sh

test_description='merge conflict with CRLF'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	echo foo >file &&
	git add file &&
	git commit -m "Initial" &&
	git tag initial &&
	git branch side &&
	echo "line from a" >file &&
	git add file &&
	git commit -m "add line from a" &&
	git tag a &&
	git checkout side &&
	echo "line from b" >file &&
	git add file &&
	git commit -m "add line from b" &&
	git tag b &&
	git checkout main
'

test_expect_success 'merge produces conflict' '
	cd repo &&
	git reset --hard a &&
	test_must_fail git merge side
'

test_expect_success 'conflict file exists' '
	cd repo &&
	test_path_is_file file &&
	grep "<<<<<<" file
'

test_done
