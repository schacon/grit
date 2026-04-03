#!/bin/sh

test_description='test git ls-files --others with non-submodule repositories'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup: directories' '
	mkdir nonrepo-no-files/ &&
	mkdir nonrepo-untracked-file &&
	: >nonrepo-untracked-file/untracked
'

test_expect_success 'ls-files --others shows untracked files in plain dirs' '
	git ls-files -o >output &&
	grep "nonrepo-untracked-file/untracked" output
'

test_expect_success 'ls-files --others does not show empty dirs' '
	git ls-files -o >output &&
	! grep "nonrepo-no-files" output
'

test_expect_success 'ls-files --others shows output file itself' '
	git ls-files -o >output &&
	grep "output" output
'

test_done
