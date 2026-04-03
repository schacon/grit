#!/bin/sh

test_description='log with pathspec filtering'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&

	echo one >file1 &&
	git add file1 &&
	test_tick &&
	git commit -q -m "add file1" &&

	echo two >file2 &&
	git add file2 &&
	test_tick &&
	git commit -q -m "add file2" &&

	mkdir sub &&
	echo three >sub/file3 &&
	git add sub/file3 &&
	test_tick &&
	git commit -q -m "add sub/file3" &&

	echo modified >file1 &&
	git add file1 &&
	test_tick &&
	git commit -q -m "modify file1"
'

test_expect_success 'log -- file shows only commits touching that file' '
	git log --format="%s" -- file1 >actual &&
	cat >expect <<-\EOF &&
	modify file1
	add file1
	EOF
	test_cmp expect actual
'

test_expect_success 'log -- directory shows commits in that dir' '
	git log --format="%s" -- sub >actual &&
	echo "add sub/file3" >expect &&
	test_cmp expect actual
'

test_expect_success 'log with multiple pathspecs' '
	git log --format="%s" -- file1 file2 >actual &&
	cat >expect <<-\EOF &&
	modify file1
	add file2
	add file1
	EOF
	test_cmp expect actual
'

test_expect_success 'log --max-count with pathspec' '
	git log -n 1 --format="%s" -- file1 >actual &&
	echo "modify file1" >expect &&
	test_cmp expect actual
'

test_done
