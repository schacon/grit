#!/bin/sh
test_description='test git log --decorate colors'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo content >file &&
	git add file &&
	test_tick &&
	git commit -m "A" &&
	git tag A
'

test_expect_success 'log --decorate shows branch and tag' '
	cd repo &&
	git log --decorate --oneline >actual &&
	grep "master" actual &&
	grep "A" actual
'

test_done
