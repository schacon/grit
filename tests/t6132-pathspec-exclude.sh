#!/bin/sh

test_description='test pathspec functionality'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	mkdir sub &&
	for f in file sub/file file2 sub/file2; do
		echo content >$f || return 1
	done &&
	git add . &&
	git commit -m base
'

test_expect_success 'log with pathspec on file' '
	cd repo &&
	echo modified >file &&
	git add file &&
	git commit -m "modify file" &&
	git log --oneline -- file >actual &&
	test_line_count = 2 actual
'

test_expect_success 'ls-files with directory pathspec' '
	cd repo &&
	git ls-files -- sub >actual &&
	test_line_count = 2 actual
'

test_expect_success 'ls-files without pathspec lists all' '
	cd repo &&
	git ls-files >actual &&
	test_line_count = 4 actual
'

test_done
