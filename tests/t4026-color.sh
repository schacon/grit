#!/bin/sh

test_description='Test diff color output'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo initial >file &&
	git add file &&
	git commit -m initial &&
	echo modified >file
'

test_expect_success 'diff --color produces colored output' '
	git diff --color=always >output &&
	test -s output
'

test_expect_success 'diff --color=never produces no escape codes' '
	git diff --color=never >output &&
	! grep "$(printf "\033")" output
'

test_expect_success 'diff --color=always produces escape codes' '
	git diff --color=always >output &&
	grep "$(printf "\033")" output
'

test_done
