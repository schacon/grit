#!/bin/sh

test_description='remerge-diff handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup basic merges' '
	git init repo &&
	cd repo &&
	test_write_lines 1 2 3 4 5 6 7 8 9 >numbers &&
	git add numbers &&
	git commit -m base &&

	git branch feature_a &&
	git branch feature_b &&

	git checkout feature_a &&
	test_write_lines 1 2 three 4 5 6 7 eight 9 >numbers &&
	git commit -a -m change_a &&

	git checkout feature_b &&
	test_write_lines 1 2 tres 4 5 6 7 8 9 >numbers &&
	git commit -a -m change_b
'

test_expect_success 'merge with conflict resolution' '
	cd repo &&
	git checkout feature_a &&
	test_must_fail git merge feature_b &&
	test_write_lines 1 2 drei 4 5 6 7 acht 9 >numbers &&
	git add numbers &&
	git commit -m "resolved"
'

test_expect_success 'diff-tree shows merge changes' '
	cd repo &&
	git diff-tree -r --name-only HEAD >actual &&
	grep "numbers" actual
'

test_done
