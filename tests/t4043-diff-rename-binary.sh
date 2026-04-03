#!/bin/sh
#
# Copyright (c) 2010 Jakub Narebski, Christian Couder
#

test_description='Move a binary file'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo
'

test_expect_success 'prepare repository' '
	cd repo &&
	echo foo > foo &&
	printf "bar\0binary" > bar &&
	git add . &&
	git commit -m "Initial commit"
'

test_expect_success 'move the files into a "sub" directory' '
	cd repo &&
	mkdir sub &&
	git mv bar foo sub/ &&
	git commit -m "Moved to sub/"
'

test_expect_success 'diff-tree -r shows renames' '
	cd repo &&
	git diff-tree -r --name-only HEAD >actual &&
	grep "sub/bar" actual &&
	grep "sub/foo" actual
'

test_expect_success 'diff-tree -p shows moved files' '
	cd repo &&
	git diff-tree -r -p HEAD >dt-output &&
	grep "sub/bar" dt-output &&
	grep "sub/foo" dt-output
'

test_done
