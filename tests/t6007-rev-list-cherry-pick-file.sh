#!/bin/sh
test_description='git rev-list --cherry-pick with file'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo a >file && git add file && test_tick && git commit -m "base" &&
	git branch other &&
	echo b >>file && git add file && test_tick && git commit -m "master change" &&
	git checkout other &&
	echo c >>file && git add file && test_tick && git commit -m "other change"
'

test_expect_success 'rev-list master..other shows diverged commits' '
	cd repo &&
	git rev-list master..other >actual &&
	test_line_count = 1 actual
'

test_expect_success 'rev-list --left-right master...other' '
	cd repo &&
	git rev-list --left-right master...other >actual &&
	test_line_count = 2 actual
'

test_done
