#!/bin/sh
# Ported from git/t/t3900-i18n-commit.sh
# Commit message handling tests

test_description='commit message handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	>F &&
	git add F &&
	git commit -m "initial commit"
'

test_expect_success 'commit with ASCII message' '
	echo content >F &&
	git add F &&
	git commit -m "Simple ASCII message" &&
	git log --format=%s -n1 >actual &&
	echo "Simple ASCII message" >expect &&
	test_cmp expect actual
'

test_expect_success 'commit with multi-line message' '
	echo more >F &&
	git add F &&
	git commit -m "Subject line

Body of the commit message
with multiple lines" &&
	git log --format=%s -n1 >actual &&
	echo "Subject line" >expect &&
	test_cmp expect actual
'

test_expect_success 'commit with -F reads from file' '
	echo even-more >F &&
	git add F &&
	echo "Message from file" >msg &&
	git commit -F msg &&
	git log --format=%s -n1 >actual &&
	echo "Message from file" >expect &&
	test_cmp expect actual
'

test_expect_success 'commit --allow-empty works' '
	git commit --allow-empty -m "empty commit" &&
	git log --format=%s -n1 >actual &&
	echo "empty commit" >expect &&
	test_cmp expect actual
'

test_expect_success 'commit --amend updates message' '
	git commit --allow-empty -m "original" &&
	git commit --amend -m "amended" &&
	git log --format=%s -n1 >actual &&
	echo "amended" >expect &&
	test_cmp expect actual
'

test_done
