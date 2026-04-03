#!/bin/sh

test_description='git apply basic and cached operations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	test_tick &&
	test_write_lines 1 2 3 4 5 6 7 >one &&
	git add one &&
	git commit -m initial &&
	git tag initial
'

test_expect_success 'apply modifies file correctly' '
	cd repo &&
	test_write_lines 1 two 3 4 5 six 7 >one &&
	git diff >P.diff &&
	git checkout -- one &&
	git apply P.diff &&
	test_write_lines 1 two 3 4 5 six 7 >expect &&
	test_cmp expect one
'

test_expect_success 'apply --cached modifies index only' '
	cd repo &&
	git checkout -- one &&
	test_write_lines 1 two 3 4 5 six 7 >one &&
	git diff >P.diff &&
	git checkout -- one &&
	git apply --cached P.diff &&
	git diff --cached >actual &&
	test_grep "+two" actual &&
	test_grep "+six" actual &&
	git reset --hard HEAD
'

test_expect_success 'apply --reverse undoes modification' '
	cd repo &&
	test_write_lines 1 two 3 4 5 six 7 >one &&
	git diff >P.diff &&
	git apply -R P.diff &&
	test_write_lines 1 2 3 4 5 6 7 >expect &&
	test_cmp expect one
'

test_expect_success 'apply --check does not modify files' '
	cd repo &&
	git checkout -- one &&
	test_write_lines 1 two 3 4 5 six 7 >one &&
	git diff >P.diff &&
	git checkout -- one &&
	git apply --check P.diff &&
	test_write_lines 1 2 3 4 5 6 7 >expect &&
	test_cmp expect one
'

test_done
