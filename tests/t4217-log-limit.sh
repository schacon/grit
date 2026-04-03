#!/bin/sh

test_description='git log with filter options limiting the output'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup test' '
	echo a >file &&
	git add file &&
	git commit -m init &&
	echo b >>file &&
	git add file &&
	git commit -m first &&
	echo c >>file &&
	git add file &&
	git commit -m second &&
	echo d >>file &&
	git add file &&
	git commit -m third
'

test_expect_success 'git log --max-count=2' '
	git log --max-count=2 --format=%s >actual &&
	cat >expect <<-\EOF &&
	third
	second
	EOF
	test_cmp expect actual
'

test_expect_success 'git log -n2' '
	git log -n2 --format=%s >actual &&
	cat >expect <<-\EOF &&
	third
	second
	EOF
	test_cmp expect actual
'

test_done
