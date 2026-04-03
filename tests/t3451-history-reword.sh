#!/bin/sh

test_description='tests for git history (log) formatting'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo A >file &&
	git add file &&
	test_tick &&
	git commit -m "commit A" &&
	git tag A &&

	echo B >file &&
	git add file &&
	test_tick &&
	git commit -m "commit B" &&
	git tag B &&

	echo C >file &&
	git add file &&
	test_tick &&
	git commit -m "commit C" &&
	git tag C
'

test_expect_success 'history --oneline shows short format' '
	git history --oneline >actual &&
	test_line_count = 3 actual
'

test_expect_success 'history --format=%H shows full hashes' '
	git history --format=%H >actual &&
	while read line; do
		test ${#line} -ge 40 || return 1
	done <actual
'

test_expect_success 'history --format=%s shows subjects' '
	git history --format=%s >actual &&
	grep "commit A" actual &&
	grep "commit B" actual &&
	grep "commit C" actual
'

test_expect_success 'history with revision range' '
	git history --oneline A..C >actual &&
	test_line_count = 2 actual
'

test_expect_success 'history with path filter' '
	echo other >other-file &&
	git add other-file &&
	test_tick &&
	git commit -m "other file" &&
	git history --oneline -- file >actual &&
	test_line_count = 3 actual
'

test_done
