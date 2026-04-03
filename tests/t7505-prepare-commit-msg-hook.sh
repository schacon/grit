#!/bin/sh
# Adapted from git/t/t7505-prepare-commit-msg-hook.sh
# Tests commit message handling

test_description='commit message handling'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init hook-repo &&
	cd hook-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo "initial" >file &&
	git add file &&
	git commit -m "initial"
'

test_expect_success 'commit -m sets message correctly' '
	cd hook-repo &&
	echo "more" >>file &&
	git add file &&
	git commit -m "specific message" &&
	git log -n 1 --format=%s >actual &&
	echo "specific message" >expect &&
	test_cmp expect actual
'

test_expect_success 'commit --allow-empty works' '
	cd hook-repo &&
	git commit --allow-empty -m "empty commit" &&
	git log -n 1 --format=%s >actual &&
	echo "empty commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'commit --amend changes message' '
	cd hook-repo &&
	git commit --amend -m "amended message" &&
	git log -n 1 --format=%s >actual &&
	echo "amended message" >expect &&
	test_cmp expect actual
'

test_expect_success 'multiple commits maintain separate messages' '
	cd hook-repo &&
	echo "c1" >>file &&
	git add file &&
	git commit -m "commit one" &&
	echo "c2" >>file &&
	git add file &&
	git commit -m "commit two" &&
	git log -n 1 --format=%s >actual &&
	echo "commit two" >expect &&
	test_cmp expect actual
'

test_done
