#!/bin/sh

test_description='CRLF merge conflict across text=auto change'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	git config core.autocrlf false &&

	echo "first line" >file &&
	echo "first line" >control_file &&
	echo "only line" >inert_file &&

	git add file control_file inert_file &&
	git commit -m "Initial" &&
	git tag initial &&
	git branch side
'

test_expect_success 'parallel non-conflicting changes merge' '
	echo "extra from main" >>control_file &&
	git add control_file &&
	git commit -m "main change" &&

	git checkout side &&
	echo "extra from side" >>inert_file &&
	git add inert_file &&
	git commit -m "side change" &&

	git checkout main &&
	git merge side &&
	grep "extra from main" control_file &&
	grep "extra from side" inert_file
'

test_expect_success 'conflicting same-line changes detected' '
	git checkout -b conflict-a initial &&
	echo "line from a" >file &&
	git add file &&
	git commit -m "a replaces file" &&

	git checkout -b conflict-b initial &&
	echo "line from b" >file &&
	git add file &&
	git commit -m "b replaces file" &&

	git checkout conflict-a &&
	test_must_fail git merge conflict-b
'

test_expect_success 'delete vs modify conflict detected' '
	git merge --abort 2>/dev/null;
	git checkout -b del-branch initial &&
	git rm file &&
	git commit -m "delete file" &&

	git checkout -b mod-branch initial &&
	echo "modified" >file &&
	git add file &&
	git commit -m "modify file" &&

	git checkout del-branch &&
	test_must_fail git merge mod-branch
'

test_done
