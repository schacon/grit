#!/bin/sh
test_description='test git rev-parse diagnosis for invalid argument'
exec </dev/null
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
. ./test-lib.sh

test_expect_success 'set up basic repo' '
	echo one > file.txt &&
	mkdir subdir &&
	echo two > subdir/file.txt &&
	echo three > subdir/file2.txt &&
	git add . &&
	git commit -m init &&
	echo four > index-only.txt &&
	git add index-only.txt &&
	echo five > disk-only.txt
'

test_expect_success 'correct file objects via HEAD:path' '
	HASH_file=$(git rev-parse HEAD:file.txt) &&
	git rev-parse HEAD:subdir/file.txt &&
	(cd subdir &&
	 git rev-parse HEAD:subdir/file2.txt &&
	 test $HASH_file = $(git rev-parse HEAD:file.txt))
'

test_expect_success 'correct relative file objects (1)' '
	git rev-parse HEAD:file.txt >expected &&
	git rev-parse HEAD:./file.txt >result &&
	test_cmp expected result
'

test_expect_success 'arg before dashdash must be a revision (missing)' '
	test_must_fail git rev-parse foobar -- 2>stderr &&
	test_grep "bad revision" stderr
'

test_expect_success 'arg before dashdash must be a revision (file)' '
	>foobar &&
	test_must_fail git rev-parse foobar -- 2>stderr &&
	test_grep "bad revision" stderr
'

test_expect_success 'reject Nth parent if N is too high' '
	test_must_fail git rev-parse HEAD^100000000000000000000000000000000
'

test_expect_success 'reject Nth ancestor if N is too high' '
	test_must_fail git rev-parse HEAD~100000000000000000000000000000000
'

test_done
