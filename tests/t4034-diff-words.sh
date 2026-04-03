#!/bin/sh

test_description='diff word-level output'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo "hello world" >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'diff shows changes' '
	echo "hello earth" >file &&
	git diff >actual &&
	grep "hello" actual &&
	grep "world\|earth" actual
'

test_expect_success 'diff --stat shows summary' '
	git diff --stat >actual &&
	grep "file" actual &&
	grep "1 file changed" actual
'

test_done
