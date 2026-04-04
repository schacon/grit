#!/bin/sh

test_description='git-diff check diffstat filepaths length when containing UTF-8 chars'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.email test@test.com &&
	git config user.name "Test User" &&
	git config core.quotePath off &&
	git commit -m "Initial commit" --allow-empty &&
	mkdir -p "d你好" &&
	touch "d你好/f再见" &&
	git add . &&
	git commit -m "Added files"
'

test_expect_success 'diff --stat shows output for UTF-8 paths' '
	git diff --stat HEAD~1 HEAD >out &&
	grep "f再见" out
'

test_expect_success 'test name-width long enough for filepath' '
	git diff --stat --stat-name-width=12 HEAD~1 HEAD >out &&
	grep "d你好/f再见 |" out
'

test_expect_success 'test name-width not long enough for dir name' '
	git diff --stat --stat-name-width=10 HEAD~1 HEAD >out &&
	grep ".../f再见  |" out
'

test_expect_success 'test name-width minimum length' '
	git diff --stat --stat-name-width=3 HEAD~1 HEAD >out &&
	grep "... |" out
'

test_done
