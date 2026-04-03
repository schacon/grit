#!/bin/sh
test_description='git log with filter options limiting the output'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup test' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo a >file &&
	git add file &&
	git commit -m init &&
	echo a >>file &&
	git add file &&
	git commit -m first &&
	echo a >>file &&
	git add file &&
	git commit -m second &&
	echo a >>file &&
	git add file &&
	git commit -m third
'

test_expect_success 'git log --max-count limits output' '
	cd repo &&
	git log --max-count=2 --format=%s >actual &&
	cat >expect <<-\EOF &&
	third
	second
	EOF
	test_cmp expect actual
'

test_expect_success 'git log -n limits output' '
	cd repo &&
	git log -n 1 --format=%s >actual &&
	echo third >expect &&
	test_cmp expect actual
'

test_done
