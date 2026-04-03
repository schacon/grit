#!/bin/sh

test_description='rebase behavior when on-disk files are broken'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m base &&
	git tag base &&

	git checkout -b branch1 &&
	echo one >file &&
	git add file &&
	test_tick &&
	git commit -m one &&

	git checkout -b branch2 base &&
	echo two >file &&
	git add file &&
	test_tick &&
	git commit -m two
'

test_expect_success 'conflicting rebase fails as expected' '
	git checkout branch2 &&
	test_must_fail git rebase branch1
'

test_expect_success 'rebase --abort after conflict restores state' '
	git rebase --abort &&
	echo two >expect &&
	test_cmp expect file
'

test_expect_success 'non-conflicting rebase succeeds' '
	git checkout -b no-conflict base &&
	echo extra >extra-file &&
	git add extra-file &&
	test_tick &&
	git commit -m extra &&
	git rebase branch1 &&
	test_path_is_file extra-file &&
	echo one >expect &&
	test_cmp expect file
'

test_expect_success 'rebase --skip skips conflicting commit' '
	git checkout branch2 &&
	test_must_fail git rebase branch1 &&
	git rebase --skip
'

test_done
