#!/bin/sh
test_description='git commit summary'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	test_seq 101 200 >file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'commit with large change succeeds' '
	cd repo &&
	test_seq 200 300 >file &&
	git add file &&
	git commit -m second
'

test_expect_success 'commit with reset and recommit' '
	cd repo &&
	git reset --hard HEAD~1 &&
	test_seq 200 300 >file &&
	git add file &&
	git commit -m "recommit"
'

test_done
