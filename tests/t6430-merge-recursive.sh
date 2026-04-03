#!/bin/sh
# Ported from git/t/t6430-merge-recursive.sh
# Tests for merge-recursive backend

test_description='merge-recursive backend test'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init recursive-repo &&
	cd recursive-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo hello >a &&
	echo hello >b &&
	echo hello >c &&
	mkdir d &&
	echo hello >d/e &&

	git add a b c d/e &&
	git commit -m initial &&

	git branch side &&

	echo "hello world" >a &&
	echo "hello world" >d/e &&
	git add a d/e &&
	git commit -m "main modifies a and d/e" &&

	git checkout side &&
	echo "goodbye" >b &&
	git add b &&
	git commit -m "side modifies b"
'

test_expect_success 'merge with no conflicts' '
	cd recursive-repo &&
	git checkout main &&
	git merge side -m "merge side" &&
	echo "goodbye" >expect &&
	test_cmp expect b &&
	echo "hello world" >expect &&
	test_cmp expect a
'

test_expect_success 'merge creates proper tree' '
	cd recursive-repo &&
	test_path_is_file a &&
	test_path_is_file b &&
	test_path_is_file c &&
	test_path_is_file d/e
'

test_expect_success 'setup conflicting merge' '
	cd recursive-repo &&
	git checkout main &&
	git branch conflict-a &&
	git branch conflict-b &&

	git checkout conflict-a &&
	echo "version A" >a &&
	git add a &&
	git commit -m "conflict-a modifies a" &&

	git checkout conflict-b &&
	echo "version B" >a &&
	git add a &&
	git commit -m "conflict-b modifies a"
'

test_expect_success 'merge with conflict fails properly' '
	cd recursive-repo &&
	git checkout conflict-a &&
	test_must_fail git merge conflict-b -m "conflict merge"
'

test_done
