#!/bin/sh

test_description='external diff interface test'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	test_tick &&
	echo initial >file &&
	git add file &&
	git commit -m initial &&

	test_tick &&
	echo second >file &&
	git add file &&
	git commit -m second &&

	test_tick &&
	echo third >file
'

test_expect_success 'GIT_EXTERNAL_DIFF environment' '
	cd repo &&
	GIT_EXTERNAL_DIFF=echo git diff >out &&
	test -s out
'

test_expect_success 'diff.external' '
	cd repo &&
	git reset --hard &&
	echo third >file &&
	test_config diff.external echo &&
	git diff >out &&
	test -s out
'

test_expect_success 'diff.external should apply only to diff' '
	cd repo &&
	test_config diff.external echo &&
	git log --max-count=1 HEAD >out &&
	grep "second" out
'

test_expect_success 'GIT_EXTERNAL_DIFF with more than one changed files' '
	cd repo &&
	rm -f .gitattributes &&
	git reset --hard &&
	echo anotherfile > file2 &&
	git add file2 &&
	git commit -m "added 2nd file" &&
	echo modified >file2 &&
	echo modified >file &&
	GIT_EXTERNAL_DIFF=echo git diff >out &&
	test -s out
'

test_done
