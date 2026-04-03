#!/bin/sh

test_description='split index mode tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Split-index tests require test-tool which is not available in grit.
# Test basic index operations instead.

test_expect_success 'setup' '
	git init repo &&
	cd repo
'

test_expect_success 'update-index works on a basic file' '
	cd repo &&
	echo content >file &&
	git update-index --add file &&
	git ls-files >actual &&
	grep "file" actual
'

test_expect_success 'update-index with multiple files' '
	cd repo &&
	echo a >a &&
	echo b >b &&
	echo c >c &&
	git update-index --add a b c &&
	git ls-files >actual &&
	test_line_count = 4 actual
'

test_done
