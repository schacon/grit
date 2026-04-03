#!/bin/sh

test_description='word diff'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup pre and post files' '
	printf "h(4)\n\na = b + c\n" >file &&
	git add file &&
	git commit -m initial &&
	printf "h(4),hh[44]\n\na = b + c\n\naa = a\n\naeff = aeff * ( aaa )\n" >file
'

test_expect_success '--word-diff=plain produces output' '
	git diff --word-diff=plain >output &&
	test -s output &&
	grep "\[-h(4)-\]" output &&
	grep "{+h(4),hh\[44\]+}" output
'

test_expect_success '--word-diff=plain --no-color produces output' '
	git diff --word-diff=plain >output &&
	test -s output
'

test_expect_success 'word diff with no newline at EOF' '
	printf "%s" "a a a a a" >pre &&
	printf "%s" "a a ab a a" >post &&
	git add pre post &&
	git commit -m setup &&
	printf "%s" "a a ab a a" >pre &&
	git diff --word-diff=plain -- pre >output &&
	grep "\[-a-\]" output &&
	grep "{+ab+}" output
'

test_done
