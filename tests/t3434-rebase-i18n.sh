#!/bin/sh
# Ported from git/t/t3434-rebase-i18n.sh
# Rebase preserves commit encoding information

test_description='rebase preserves commit messages'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	test_commit base &&
	test_commit upstream-change &&
	git checkout -b topic base &&
	test_commit topic-change
'

test_expect_success 'rebase preserves commit message' '
	git checkout topic &&
	git rebase master &&
	git log --format=%s -n1 >actual &&
	echo "topic-change" >expect &&
	test_cmp expect actual
'

test_expect_success 'rebase preserves author info' '
	author_name=$(git log --format=%an -n1) &&
	author_email=$(git log --format=%ae -n1) &&
	test "$author_name" = "A U Thor" &&
	test "$author_email" = "author@example.com"
'

test_done
