#!/bin/sh
# Ported from git/t/t3437-rebase-fixup-options.sh
# Basic rebase skip and abort tests

test_description='rebase skip and abort'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo base >file &&
	git add file &&
	git commit -m "base" &&
	git tag base &&

	git checkout -b side &&
	echo side1 >file &&
	git add file &&
	git commit -m "side1" &&

	echo side2 >file &&
	git add file &&
	git commit -m "side2" &&
	git tag side-tip &&

	git checkout master &&
	echo main1 >file &&
	git add file &&
	git commit -m "main1"
'

test_expect_success 'rebase --abort returns to original state' '
	git checkout -b abort-test side-tip &&
	old=$(git rev-parse HEAD) &&
	test_must_fail git rebase master &&
	git rebase --abort &&
	new=$(git rev-parse HEAD) &&
	test "$old" = "$new"
'

test_expect_success 'rebase detects conflict' '
	git checkout -b conflict-test side-tip &&
	test_must_fail git rebase master
'

test_expect_success 'after conflict abort, branch is restored' '
	git rebase --abort &&
	branch=$(git symbolic-ref --short HEAD) &&
	test "$branch" = "conflict-test"
'

test_done
