#!/bin/sh

test_description='pathspec with wildcards and literal characters'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	mkdir dir &&
	echo content >dir/file.txt &&
	echo content >dir/file.rb &&
	echo content >top.txt &&
	git add . &&
	git commit -m initial
'

test_expect_success 'ls-files with directory pathspec' '
	cd repo &&
	git ls-files -- dir >output &&
	test_line_count = 2 output
'

test_expect_success 'ls-files shows all files' '
	cd repo &&
	git ls-files >output &&
	test_line_count = 3 output
'

test_expect_success 'log with pathspec' '
	cd repo &&
	git log --oneline -- dir >output &&
	test_line_count = 1 output
'

test_done
