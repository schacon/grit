#!/bin/sh
test_description='pre-commit and pre-merge-commit hooks'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "content" >file &&
	git add file &&
	git commit -m "initial"
'

test_expect_success 'with no hook, commit succeeds' '
	cd repo &&
	echo "more" >>file &&
	git add file &&
	git commit -m "no hook"
'

test_expect_success 'commit with allow-empty' '
	cd repo &&
	git commit --allow-empty -m "empty commit"
'

test_done
