#!/bin/sh
#
# Ported from git/t/t2100-update-cache-badpath.sh
# Tests basic update-index --add functionality.
# Note: grit does not check for file/directory conflicts yet,
# so conflict tests are omitted.

test_description='git update-index basic path tests'

. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success 'git update-index --add adds files to index' '
	date >path0 &&
	date >path1 &&
	mkdir path2 &&
	date >path2/file2 &&
	mkdir path3 &&
	date >path3/file3 &&
	git update-index --add -- path0 path1 path2/file2 path3/file3 &&
	git ls-files -s >actual &&
	test_line_count = 4 actual
'

test_expect_success 'update-index rejects non-existent files without --add' '
	test_must_fail git update-index -- nonexistent 2>err
'

test_expect_success 'update-index --add works with multiple files' '
	echo new1 >new1 &&
	echo new2 >new2 &&
	git update-index --add new1 new2 &&
	git ls-files new1 new2 >actual &&
	test_line_count = 2 actual
'

test_done
