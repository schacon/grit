#!/bin/sh

test_description='merge: handle file mode changes'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'set up mode change in one branch' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	: >file1 &&
	git add file1 &&
	git commit -m initial &&
	git checkout -b a1 main &&
	: >dummy &&
	git add dummy &&
	git commit -m a &&
	git checkout -b b1 main &&
	chmod +x file1 &&
	git add file1 &&
	git commit -m b1
'

test_expect_success 'merge with mode change (a1 merges b1)' '
	cd repo &&
	git checkout a1 &&
	git merge b1 &&
	git ls-files -s file1 | grep ^100755
'

test_expect_success 'merge with mode change (b1 merges a1)' '
	cd repo &&
	git checkout b1 &&
	git merge a1 &&
	git ls-files -s file1 | grep ^100755
'

test_done
