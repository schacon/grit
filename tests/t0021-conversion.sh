#!/bin/sh

test_description='blob conversion via gitattributes'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet support clean/smudge filters or ident keyword expansion.
# These tests verify basic setup but mark filter-dependent tests as expected failures.

test_expect_success 'setup' '
	git init &&
	printf "hello\r\n" >crlf_file &&
	printf "hello\n" >lf_file &&
	git add crlf_file lf_file &&
	git commit -m "initial with mixed line endings"
'

test_expect_success 'autocrlf=false preserves LF on input' '
	git config core.autocrlf false &&
	echo "test content" >newfile &&
	git add newfile &&
	git commit -m "add newfile" &&
	git cat-file blob HEAD:newfile >actual &&
	echo "test content" >expect &&
	test_cmp expect actual
'

test_expect_success 'clean filter converts on input' '
	git config filter.test.clean "sed s/.*/CLEAN/" &&
	echo "* filter=test" >.gitattributes &&
	echo "original" >filtered_file &&
	git add filtered_file &&
	echo "CLEAN" >expect &&
	git cat-file blob :filtered_file >actual &&
	test_cmp expect actual
'

test_expect_success 'smudge filter converts on output' '
	git config filter.test.smudge "sed s/.*/SMUDGE/" &&
	echo "* filter=test" >.gitattributes &&
	echo "original" >smudge_file &&
	git add smudge_file &&
	git commit -m "add smudge_file" &&
	rm smudge_file &&
	git checkout -- smudge_file &&
	echo "SMUDGE" >expect &&
	test_cmp expect smudge_file
'

test_expect_success 'ident keyword expansion on checkout' '
	echo "*.txt ident" >.gitattributes &&
	echo "\$Id\$" >ident_file.txt &&
	git add .gitattributes ident_file.txt &&
	git commit -m "add ident file" &&
	rm ident_file.txt &&
	git checkout -- ident_file.txt &&
	grep "\$Id:" ident_file.txt
'

test_done
