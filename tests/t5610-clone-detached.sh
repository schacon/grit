#!/bin/sh
# Ported from git/t/t5610-clone-detached.sh
# test cloning a repository with detached HEAD

test_description='test cloning a repository with detached HEAD'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo one >file &&
	git add file &&
	git commit -m one &&
	echo two >file &&
	git commit -a -m two &&
	git tag two &&
	echo three >file &&
	git commit -a -m three
'

test_expect_success 'clone repo on branch' '
	git clone "$TRASH_DIRECTORY" cloned-on-branch
'

test_expect_success 'cloned HEAD matches (on branch)' '
	echo three >expect &&
	git --git-dir=cloned-on-branch/.git log -n 1 --format=%s >actual &&
	test_cmp expect actual
'

test_expect_success 'clone repo (branch explicitly)' '
	git clone --branch main "$TRASH_DIRECTORY" cloned-explicit
'

test_expect_success 'cloned HEAD matches (explicit branch)' '
	echo three >expect &&
	git --git-dir=cloned-explicit/.git log -n 1 --format=%s >actual &&
	test_cmp expect actual
'

test_expect_success 'clone bare' '
	git clone --bare "$TRASH_DIRECTORY" cloned-bare.git
'

test_expect_success 'cloned bare HEAD matches' '
	echo three >expect &&
	git --git-dir=cloned-bare.git log -n 1 --format=%s >actual &&
	test_cmp expect actual
'

test_done
