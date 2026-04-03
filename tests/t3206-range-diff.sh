#!/bin/sh

test_description='range-diff tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m "base" &&
	git tag base &&

	git checkout -b topic &&
	echo one >file &&
	git add file &&
	test_tick &&
	git commit -m "first change" &&

	echo two >file &&
	git add file &&
	test_tick &&
	git commit -m "second change" &&
	git tag topic-end &&

	git checkout main &&
	echo main-change >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m "main advance" &&

	git checkout -b rebased-topic &&
	git cherry-pick topic~1 &&
	git cherry-pick topic &&
	git tag rebased-topic-end
'

test_expect_success 'range-diff with two ranges' '
	git range-diff base..topic-end main..rebased-topic-end >actual &&
	test -s actual
'

test_expect_success 'range-diff output shows commits' '
	grep "first change" actual &&
	grep "second change" actual
'

test_expect_success 'range-diff identical ranges show equal sign' '
	git range-diff base..topic-end base..topic-end >actual &&
	grep "=" actual
'

test_done
