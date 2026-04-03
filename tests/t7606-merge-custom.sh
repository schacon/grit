#!/bin/sh

test_description='git merge with custom options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init merge-custom &&
	cd merge-custom &&
	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m base &&
	git tag base &&

	git checkout -b side &&
	echo side >side-file &&
	git add side-file &&
	test_tick &&
	git commit -m side &&
	git tag side-tag &&

	git checkout master &&
	echo main >main-file &&
	git add main-file &&
	test_tick &&
	git commit -m "main advance"
'

test_expect_success 'merge with --no-ff creates merge commit' '
	cd merge-custom &&
	git checkout master &&
	git merge --no-ff side &&
	test $(git rev-parse HEAD^1) = $(git rev-parse HEAD~1) &&
	test $(git rev-parse HEAD^2) = $(git rev-parse side)
'

test_expect_success 'merge --no-commit stages but does not commit' '
	cd merge-custom &&
	git checkout -b nocommit-test base &&
	echo main2 >main-file2 &&
	git add main-file2 &&
	test_tick &&
	git commit -m "main advance 2" &&
	git merge --no-commit side &&
	test_path_is_file .git/MERGE_HEAD &&
	test_path_is_file side-file
'

test_expect_success 'merge --ff-only succeeds on fast-forward' '
	cd merge-custom &&
	git checkout -b ff-test base &&
	git merge --ff-only side &&
	test $(git rev-parse HEAD) = $(git rev-parse side)
'

test_done
