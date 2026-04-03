#!/bin/sh
test_description='git rev-list --bisect'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo a >file && git add file && test_tick && git commit -m "first" &&
	echo b >>file && git add file && test_tick && git commit -m "second" &&
	echo c >>file && git add file && test_tick && git commit -m "third" &&
	echo d >>file && git add file && test_tick && git commit -m "fourth"
'

test_expect_success 'rev-list --count shows total' '
	cd repo &&
	git rev-list --count HEAD >actual &&
	echo 4 >expect &&
	test_cmp expect actual
'

test_done
