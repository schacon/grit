#!/bin/sh

test_description='diff stat output'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	mkdir sub &&
	echo content >sub/file1 &&
	echo content >sub/file2 &&
	echo other >top &&
	git add . &&
	git commit -m initial
'

test_expect_success 'diff --stat shows changed files' '
	echo changed >sub/file1 &&
	echo changed >top &&
	git diff --stat >actual &&
	grep "sub/file1" actual &&
	grep "top" actual &&
	grep "2 files changed" actual
'

test_expect_success 'diff --numstat shows numeric stats' '
	git diff --numstat >actual &&
	grep "sub/file1" actual &&
	grep "top" actual
'

test_done
