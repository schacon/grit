#!/bin/sh

test_description='diff --relative tests

Upstream git test t4045. Most tests require --relative, --raw, --no-abbrev,
and -C (run from subdir) which grit does not fully support yet.'

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.email test@test.com &&
	git config user.name "Test User" &&
	git commit --allow-empty -m empty &&
	echo content >file1 &&
	mkdir subdir &&
	echo "other content" >subdir/file2 &&
	git add . &&
	git commit -m one
'

test_expect_success '-p --relative=subdir/' '
	cd repo &&
	git diff -p --relative=subdir/ HEAD^ >actual &&
	grep "file2" actual
'

test_expect_success '--numstat --relative=subdir/' '
	cd repo &&
	git diff --numstat --relative=subdir/ HEAD^ >actual &&
	grep "file2" actual
'

test_expect_success '--stat --relative=subdir/' '
	cd repo &&
	git diff --stat --relative=subdir/ HEAD^ >actual &&
	grep "file2" actual
'

test_expect_failure '--raw --relative=subdir/ (not implemented)' '
	cd repo &&
	git diff --raw --relative=subdir/ HEAD^ >actual &&
	grep "file2" actual
'

test_expect_success '--relative from subdir' '
	cd repo/subdir &&
	git diff -p --relative HEAD^ >actual &&
	grep "file2" actual
'

test_expect_success '--no-relative' '
	cd repo &&
	git diff -p --no-relative HEAD^ >actual &&
	grep "subdir/file2" actual
'

test_expect_success 'config diff.relative' '
	cd repo/subdir &&
	git -c diff.relative=true diff -p HEAD^ >actual &&
	grep "file2" actual
'

test_done
