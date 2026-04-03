#!/bin/sh
test_description='git apply with new style GNU diff with empty context'
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success setup '
	test_write_lines "" "" A B C "" >file1 &&
	cat file1 >file1.orig &&
	git add file1 &&
	sed -e "/^B/d" <file1.orig >file1 &&
	cat file1 >file1.mods &&
	git diff |
	sed -e "s/^ \$//" >diff.output &&
	cat file1.orig >file1 &&
	git update-index file1
'

test_expect_success 'apply --numstat' '
	git apply --numstat diff.output >actual &&
	echo "0	1	file1" >expect &&
	test_cmp expect actual
'

test_expect_success 'apply works' '
	cat file1.orig >file1 &&
	git update-index file1 &&
	git apply diff.output &&
	test_cmp file1.mods file1
'

test_done
