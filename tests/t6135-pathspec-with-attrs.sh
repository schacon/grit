#!/bin/sh

test_description='pathspec with attributes'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	mkdir sub &&
	echo content >file.txt &&
	echo content >file.rb &&
	echo content >sub/file.txt &&
	echo content >sub/file.rb &&
	git add . &&
	git commit -m initial
'

test_expect_success 'ls-files with directory pathspec' '
	cd repo &&
	git ls-files -- sub >output &&
	test_line_count = 2 output
'

test_expect_success 'log with simple pathspec' '
	cd repo &&
	git log --oneline -- file.txt >output &&
	test_line_count = 1 output
'

test_done
