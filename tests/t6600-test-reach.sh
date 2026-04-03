#!/bin/sh
# Ported from git/t/t6600-test-reach.sh
# Tests basic commit reachability

test_description='basic commit reachability tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup linear history' '
	git init reach-repo &&
	cd reach-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo "1" >file &&
	git add file &&
	git commit -m "commit-1" &&
	git tag commit-1 &&

	echo "2" >file &&
	git add file &&
	git commit -m "commit-2" &&
	git tag commit-2 &&

	echo "3" >file &&
	git add file &&
	git commit -m "commit-3" &&
	git tag commit-3 &&

	echo "4" >file &&
	git add file &&
	git commit -m "commit-4" &&
	git tag commit-4
'

test_expect_success 'merge-base of linear commits' '
	cd reach-repo &&
	git merge-base commit-1 commit-4 >actual &&
	git rev-parse commit-1 >expect &&
	test_cmp expect actual
'

test_expect_success 'merge-base of adjacent commits' '
	cd reach-repo &&
	git merge-base commit-2 commit-3 >actual &&
	git rev-parse commit-2 >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-list can limit to reachable commits' '
	cd reach-repo &&
	git rev-list commit-3 >list &&
	test_line_count = 3 list
'

test_expect_success 'setup branching history' '
	cd reach-repo &&
	git checkout commit-2 &&
	git checkout -b side &&
	echo "side" >side-file &&
	git add side-file &&
	git commit -m "side-1" &&
	git tag side-1
'

test_expect_success 'merge-base of diverged branches' '
	cd reach-repo &&
	git merge-base commit-4 side-1 >actual &&
	git rev-parse commit-2 >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-list with exclusion' '
	cd reach-repo &&
	git rev-list commit-4 ^commit-2 >list &&
	test_line_count = 2 list
'

test_done
