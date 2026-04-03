#!/bin/sh

test_description='git merge-tree (3-argument form)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	test_write_lines 1 2 3 4 5 >numbers &&
	echo hello >greeting &&
	git add numbers greeting &&
	test_tick &&
	git commit -m initial &&
	git tag base &&

	git checkout -b side1 base &&
	test_write_lines 1 2 3 4 5 6 >numbers &&
	echo hi >greeting &&
	git add numbers greeting &&
	test_tick &&
	git commit -m modify-stuff &&

	git checkout -b side2 base &&
	test_write_lines 0 1 2 3 4 5 >numbers &&
	echo yo >greeting &&
	git add numbers greeting &&
	test_tick &&
	git commit -m other-modifications
'

test_expect_success 'merge-tree shows conflict for overlapping changes' '
	cd repo &&
	git merge-tree base side1 side2 >output &&
	test_grep "changed in both" output
'

test_expect_success 'merge-tree shows conflict markers' '
	cd repo &&
	git merge-tree base side1 side2 >output &&
	test_grep "<<<<<<" output &&
	test_grep ">>>>>>" output
'

test_expect_success 'merge-tree with identical changes is clean' '
	cd repo &&
	git checkout -b same1 base &&
	echo identical >greeting &&
	git add greeting &&
	git commit -m same-change &&

	git checkout -b same2 base &&
	echo identical >greeting &&
	git add greeting &&
	git commit -m same-change-2 &&

	git merge-tree base same1 same2 >output &&
	# When changes are identical, output should not show conflict
	! test_grep "changed in both" output
'

test_done
