#!/bin/sh

test_description='Test reflog exists'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file.t &&
	git add file.t &&
	test_tick &&
	git commit -m A &&
	git tag A &&
	mkdir -p .git/logs/refs/heads &&
	hash=$(git rev-parse HEAD) &&
	echo "0000000000000000000000000000000000000000 $hash 1112911993 -0700	commit: A" >.git/logs/refs/heads/main
'

test_expect_success 'reflog exists works' '
	git reflog exists refs/heads/main &&
	test_must_fail git reflog exists refs/heads/nonexistent
'

test_expect_success 'reflog exists works with a "--" delimiter' '
	git reflog exists -- refs/heads/main &&
	test_must_fail git reflog exists -- refs/heads/nonexistent
'

test_done
