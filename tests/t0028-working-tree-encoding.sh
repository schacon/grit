#!/bin/sh

test_description='working-tree-encoding conversion via gitattributes'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet support working-tree-encoding gitattribute.
# All encoding conversion tests are expected failures.

test_expect_success 'setup' '
	git init &&
	git config core.eol lf
'

test_expect_failure 'working-tree-encoding UTF-16 round-trips through git' '
	echo "*.utf16 text working-tree-encoding=utf-16" >.gitattributes &&
	printf "hello\\nworld\\n" >test.utf8.raw &&
	printf "hello\\nworld\\n" | iconv -f UTF-8 -t UTF-16 >test.utf16 &&
	git add .gitattributes test.utf16 &&
	git commit -m initial &&
	git cat-file -p HEAD:test.utf16 >actual &&
	test_cmp test.utf8.raw actual
'

test_expect_failure 'working-tree-encoding error on invalid encoding' '
	echo "*.bad text working-tree-encoding=INVALID-ENCODING" >.gitattributes &&
	echo "content" >test.bad &&
	git add .gitattributes &&
	test_must_fail git add test.bad 2>err &&
	test_grep "encoding" err
'

test_done
