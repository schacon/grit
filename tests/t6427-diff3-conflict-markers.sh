#!/bin/sh
# Ported from git/t/t6427-diff3-conflict-markers.sh
# Tests conflict markers during merge.

test_description='diff3-style conflict markers during merge'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: create conflicting branches' '
	git init conflict-repo &&
	cd conflict-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo "base content" >file &&
	git add file &&
	git commit -m "base" &&

	git branch sideA &&
	git branch sideB &&

	git checkout sideA &&
	echo "content from A" >file &&
	git add file &&
	git commit -m "change on A" &&

	git checkout sideB &&
	echo "content from B" >file &&
	git add file &&
	git commit -m "change on B"
'

test_expect_success 'merge produces conflict markers' '
	cd conflict-repo &&
	git checkout sideA &&
	test_must_fail git merge sideB -m "merge" &&
	test_grep "=======" file
'

test_expect_success 'conflict markers contain branch info' '
	cd conflict-repo &&
	test_grep "<<<<<<" file &&
	test_grep ">>>>>>" file
'

test_expect_success 'merge --abort cleans up conflict' '
	cd conflict-repo &&
	git merge --abort &&
	echo "content from A" >expect &&
	test_cmp expect file
'

test_done
