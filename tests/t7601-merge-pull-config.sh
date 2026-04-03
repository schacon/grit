#!/bin/sh
test_description='merge pull config'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo base >file &&
	git add file &&
	git commit -m "base" &&
	git branch other &&
	echo main >>file &&
	git add file &&
	git commit -m "main change" &&
	git checkout other &&
	echo other >other-file &&
	git add other-file &&
	git commit -m "other change"
'

test_expect_success 'merge --no-commit' '
	cd repo &&
	git checkout master &&
	git merge --no-commit other &&
	test -f .git/MERGE_HEAD
'

test_done
