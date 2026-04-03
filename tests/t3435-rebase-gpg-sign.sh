#!/bin/sh
# Ported from git/t/t3435-rebase-gpg-sign.sh
# Basic rebase tests (gpg signing not tested)

test_description='rebase basic commit handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	test_commit initial &&
	test_commit second &&
	git checkout -b side initial &&
	test_commit side-work
'

test_expect_success 'rebase creates new commits' '
	git checkout side &&
	old=$(git rev-parse HEAD) &&
	git rebase master &&
	new=$(git rev-parse HEAD) &&
	test "$old" != "$new"
'

test_expect_success 'rebased commit has correct parent' '
	parent=$(git rev-parse HEAD^) &&
	master_tip=$(git rev-parse master) &&
	test "$parent" = "$master_tip"
'

test_expect_success 'rebased commit preserves author' '
	git log --format=%an -n1 >actual &&
	echo "A U Thor" >expect &&
	test_cmp expect actual
'

test_done
