#!/bin/sh

test_description='test local clone'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'preparing origin repository' '
	git init &&
	: >file && git add . && git commit -m1 &&
	git clone --bare . a.git
'

test_expect_success 'local clone without .git suffix' '
	git clone a.git b &&
	(cd b && git fetch)
'

test_expect_success 'local clone with .git suffix' '
	git clone a.git c &&
	(cd c && git fetch)
'

test_expect_success 'clone from bare repo' '
	git clone a.git d &&
	test_path_is_dir d/.git
'

test_expect_success 'clone with explicit local path' '
	git clone ./a.git e &&
	test_path_is_dir e/.git
'

test_expect_success 'local clone auto-appends .git suffix' '
	git clone a z &&
	test -d z/.git
'

test_expect_success 'cloned repo is functional' '
	(cd b &&
	 echo test >newfile &&
	 git add newfile &&
	 git commit -m "test commit")
'

test_done
