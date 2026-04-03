#!/bin/sh
# Adapted from git/t/t6436-merge-overwrite.sh
# Tests merge behavior with file changes

test_description='git merge - file handling'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init overwrite &&
	cd overwrite &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo "c0" >c0.c &&
	git add c0.c &&
	git commit -m "c0" &&
	git tag c0 &&

	echo "c1" >c1.c &&
	git add c1.c &&
	git commit -m "c1" &&
	git tag c1 &&

	git reset --hard c0 &&
	echo "c2" >c2.c &&
	git add c2.c &&
	git commit -m "c2" &&
	git tag c2
'

test_expect_success 'merge brings in new files' '
	cd overwrite &&
	git reset --hard c1 &&
	git merge c2 -m "merge c2" &&
	test_path_is_file c2.c &&
	test_path_is_file c1.c
'

test_expect_success 'merge does not lose existing files' '
	cd overwrite &&
	test_path_is_file c0.c &&
	echo "c0" >expect &&
	test_cmp expect c0.c
'

test_expect_success 'merge with conflict on same file' '
	cd overwrite &&
	git checkout c1 &&
	echo "modified c1" >c0.c &&
	git add c0.c &&
	git commit -m "modify c0 on c1 side" &&
	git tag c1mod &&

	git checkout c2 &&
	echo "also modified c1" >c0.c &&
	git add c0.c &&
	git commit -m "modify c0 on c2 side" &&

	test_must_fail git merge c1mod -m "conflict merge"
'

test_done
