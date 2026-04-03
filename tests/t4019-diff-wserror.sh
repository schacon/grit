#!/bin/sh

test_description='diff whitespace error detection'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	git config core.whitespace "trailing-space,space-before-tab" &&
	echo "a" >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'diff detects trailing whitespace' '
	printf "a \n" >file &&
	git diff >actual &&
	grep "^+" actual
'

test_expect_success 'diff detects added lines' '
	echo "b" >>file &&
	git diff >actual &&
	grep "^+b" actual
'

test_done
