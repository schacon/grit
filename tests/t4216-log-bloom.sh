#!/bin/sh

test_description='git log with various options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo initial >file1 &&
	git add file1 &&
	git commit -m "initial" &&
	git tag initial &&

	echo second >file2 &&
	git add file2 &&
	git commit -m "add file2" &&

	echo third >file3 &&
	git add file3 &&
	git commit -m "add file3" &&

	echo modified >file1 &&
	git commit -a -m "modify file1"
'

test_expect_success 'log shows all commits' '
	cd repo &&
	git log --oneline >output &&
	test_line_count = 4 output
'

test_expect_success 'log with pathspec limits output' '
	cd repo &&
	git log --oneline -- file1 >output &&
	test_line_count = 2 output
'

test_expect_success 'log -n limits output' '
	cd repo &&
	git log --oneline -n 2 >output &&
	test_line_count = 2 output
'

test_expect_success 'log --format shows custom format' '
	cd repo &&
	git log --format="%s" -n 1 >output &&
	echo "modify file1" >expect &&
	test_cmp expect output
'

test_expect_success 'log --reverse shows oldest first' '
	cd repo &&
	git log --format="%s" --reverse >output &&
	head -1 output >first &&
	echo "initial" >expect &&
	test_cmp expect first
'

test_expect_success 'log with author filter' '
	cd repo &&
	git log --format="%s" --author="A U Thor" >output &&
	test_line_count = 4 output
'

test_done
