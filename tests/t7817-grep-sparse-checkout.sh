#!/bin/sh
# Ported from upstream git t7817-grep-sparse-checkout.sh

test_description='grep with path patterns'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init grep-sparse &&
	cd grep-sparse &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	mkdir dir1 dir2 &&
	echo "content in dir1" >dir1/file1 &&
	echo "content in dir2" >dir2/file2 &&
	echo "root content" >root.txt &&
	git add . &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'grep across all paths' '
	cd grep-sparse &&
	git grep "content" >actual &&
	test $(wc -l <actual) -eq 3
'

test_expect_success 'grep with specific filename' '
	cd grep-sparse &&
	git grep "content" -- root.txt >actual &&
	test $(wc -l <actual) -eq 1 &&
	grep "root.txt" actual
'

test_expect_success 'grep -l lists all matching files' '
	cd grep-sparse &&
	git grep -l "content" >actual &&
	test $(wc -l <actual) -eq 3
'

test_expect_success 'grep -c counts per file' '
	cd grep-sparse &&
	git grep -c "content" >actual &&
	test $(wc -l <actual) -eq 3
'

test_expect_success 'grep in HEAD' '
	cd grep-sparse &&
	git grep "content" HEAD >actual &&
	test $(wc -l <actual) -eq 3
'

test_done
