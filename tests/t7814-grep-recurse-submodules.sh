#!/bin/sh

test_description='Test grep in repos with submodule-like structure'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init grep-sub &&
	cd grep-sub &&
	echo "(1|2)d(3|4)" >a &&
	mkdir b &&
	echo "(3|4)" >b/b &&
	git add a b &&
	test_tick &&
	git commit -m "add a and b"
'

test_expect_success 'grep finds pattern in files' '
	cd grep-sub &&
	git grep -e "(3|4)" >actual &&
	grep "a:" actual &&
	grep "b/b:" actual
'

test_expect_success 'grep with fixed strings' '
	cd grep-sub &&
	git grep -F "(3|4)" >actual &&
	grep "a:" actual &&
	grep "b/b:" actual
'

test_expect_success 'grep with line numbers' '
	cd grep-sub &&
	git grep -n "(3|4)" >actual &&
	grep "a:1:" actual &&
	grep "b/b:1:" actual
'

test_expect_success 'grep with count' '
	cd grep-sub &&
	git grep -c "(3|4)" >actual &&
	grep "a:1" actual &&
	grep "b/b:1" actual
'

test_expect_success 'grep with files-with-matches' '
	cd grep-sub &&
	git grep -l "(3|4)" >actual &&
	grep "^a$" actual &&
	grep "^b/b$" actual
'

test_expect_success 'grep case insensitive' '
	cd grep-sub &&
	echo "HELLO world" >case-file &&
	git add case-file &&
	test_tick &&
	git commit -m "add case-file" &&
	git grep -i "hello" >actual &&
	grep "case-file" actual
'

test_done
