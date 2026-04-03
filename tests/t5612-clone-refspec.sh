#!/bin/sh

test_description='test refspec written by clone-command'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo one >file &&
	git add file &&
	git commit -m one &&
	echo two >file &&
	git commit -a -m two &&
	git tag two &&
	echo three >file &&
	git commit -a -m three &&
	git checkout -b side &&
	echo four >file &&
	git commit -a -m four &&
	git checkout main
'

test_expect_success 'default clone has correct refspec' '
	git clone . dir_all &&
	(
		cd dir_all &&
		echo "+refs/heads/*:refs/remotes/origin/*" >expect &&
		git config remote.origin.fetch >actual &&
		test_cmp expect actual
	)
'

test_expect_failure 'clone --single-branch with default HEAD' '
	rm -rf dir_main &&
	git clone --single-branch . dir_main &&
	(
		cd dir_main &&
		echo "+refs/heads/main:refs/remotes/origin/main" >expect &&
		git config remote.origin.fetch >actual &&
		test_cmp expect actual
	)
'

test_expect_failure 'clone --single-branch --branch side' '
	rm -rf dir_side &&
	git clone --single-branch --branch side . dir_side &&
	(
		cd dir_side &&
		echo "+refs/heads/side:refs/remotes/origin/side" >expect &&
		git config remote.origin.fetch >actual &&
		test_cmp expect actual
	)
'

test_done
