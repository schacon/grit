#!/bin/sh
# Ported from git/t/t3428-rebase-signoff.sh
# Basic rebase tests

test_description='git rebase basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	git commit --allow-empty -m "Initial empty commit" &&
	echo a >file &&
	git add file &&
	test_tick &&
	git commit -m "first" &&
	git tag first &&
	echo b >file &&
	git add file &&
	test_tick &&
	git commit -m "second" &&
	git tag second &&
	git checkout -b topic first &&
	echo content >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m "topic-change" &&
	git tag topic-change
'

test_expect_success 'basic rebase works' '
	git checkout topic &&
	git rebase master &&
	git log --format=%s -n1 >actual &&
	echo "topic-change" >expect &&
	test_cmp expect actual &&
	test_path_is_file file2 &&
	test "$(cat file)" = "b"
'

test_expect_success 'rebase onto specific commit' '
	git checkout -b topic2 first &&
	echo data >file3 &&
	git add file3 &&
	test_tick &&
	git commit -m "another-change" &&
	git rebase --onto second first &&
	git log --format=%s -n1 >actual &&
	echo "another-change" >expect &&
	test_cmp expect actual &&
	test "$(cat file)" = "b" &&
	test_path_is_file file3
'

test_done
