#!/bin/sh
# Ported from git/t/t6200-fmt-merge-msg.sh
# fmt-merge-msg test

test_description='fmt-merge-msg test'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo one >one &&
	git add one &&
	test_tick &&
	git commit -m "Initial" &&

	git checkout -b left &&
	echo "l1" >one &&
	test_tick &&
	git commit -a -m "Left #1" &&

	git checkout main &&
	git checkout -b right &&
	echo "r1" >two &&
	git add two &&
	test_tick &&
	git commit -m "Right #1" &&
	git checkout main
'

test_expect_success 'fmt-merge-msg with crafted FETCH_HEAD' '
	left_oid=$(git rev-parse left) &&
	printf "%s\t\tbranch '\''left'\'' of .\n" "$left_oid" >.git/FETCH_HEAD &&
	git fmt-merge-msg -F .git/FETCH_HEAD >actual &&
	test -s actual &&
	test_grep "left" actual
'

test_expect_success 'fmt-merge-msg with --message' '
	right_oid=$(git rev-parse right) &&
	printf "%s\t\tbranch '\''right'\'' of .\n" "$right_oid" >.git/FETCH_HEAD &&
	git fmt-merge-msg --message "Custom merge" -F .git/FETCH_HEAD >actual &&
	test -s actual &&
	test_grep "Custom merge" actual
'

test_expect_success 'fmt-merge-msg with --into-name' '
	left_oid=$(git rev-parse left) &&
	printf "%s\t\tbranch '\''left'\'' of .\n" "$left_oid" >.git/FETCH_HEAD &&
	git fmt-merge-msg --into-name develop -F .git/FETCH_HEAD >actual &&
	test -s actual &&
	test_grep "develop" actual
'

test_expect_success 'fmt-merge-msg with stdin' '
	right_oid=$(git rev-parse right) &&
	printf "%s\t\tbranch '\''right'\'' of .\n" "$right_oid" >fh &&
	git fmt-merge-msg <fh >actual &&
	test -s actual
'

test_done
