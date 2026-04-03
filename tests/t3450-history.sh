#!/bin/sh

test_description='tests for git history (log) command'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo first >file &&
	git add file &&
	test_tick &&
	git commit -m "first commit" &&
	git tag first &&

	echo second >file &&
	git add file &&
	test_tick &&
	git commit -m "second commit" &&
	git tag second &&

	echo third >file &&
	git add file &&
	test_tick &&
	git commit -m "third commit" &&
	git tag third
'

test_expect_success 'history shows commits (forwarded to git log)' '
	git history --oneline >actual &&
	test_line_count = 3 actual
'

test_expect_success 'history with max-count limits output' '
	git history --max-count=1 --oneline >actual &&
	test_line_count = 1 actual
'

test_expect_success 'history with format shows custom output' '
	git history --format=%s --max-count=1 >actual &&
	grep "third commit" actual
'

test_done
