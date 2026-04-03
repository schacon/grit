#!/bin/sh

test_description='git rebase environment'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	test_commit one &&
	test_commit two &&
	test_commit three
'

test_expect_success 'basic rebase works' '
	cd repo &&
	git checkout -b side HEAD~1 &&
	test_commit side-change &&
	git rebase master
'

test_done
