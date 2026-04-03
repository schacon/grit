#!/bin/sh

test_description='apply with fuzz and offset'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	test_write_lines 1 2 3 4 5 6 7 8 9 10 11 12 >file &&
	cp file original &&
	git add file &&
	test_write_lines 1 2 3 4 5 6 7 a b c d e 8 9 10 11 12 >file &&
	cat file >expect &&
	git diff >O0.diff
'

test_expect_success 'unmodified patch' '
	cp original file &&
	git apply O0.diff &&
	test_cmp expect file
'

test_done
