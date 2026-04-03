#!/bin/sh
# Ported from upstream git t7812-grep-icase-non-ascii.sh

test_description='grep icase on non-ASCII'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init grep-icase &&
	cd grep-icase &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo "Hello World" >file &&
	echo "HELLO WORLD" >>file &&
	echo "hello world" >>file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'grep -i finds case-insensitive matches' '
	cd grep-icase &&
	git grep -i "hello" >actual &&
	test $(wc -l <actual) -eq 3
'

test_expect_success 'grep without -i is case-sensitive' '
	cd grep-icase &&
	git grep "Hello" >actual &&
	test $(wc -l <actual) -eq 1
'

test_expect_success 'grep -i on committed content' '
	cd grep-icase &&
	git grep -i "WORLD" HEAD >actual &&
	test $(wc -l <actual) -eq 3
'

test_expect_success 'grep with mixed case pattern' '
	cd grep-icase &&
	git grep -i "hElLo WoRlD" >actual &&
	test $(wc -l <actual) -eq 3
'

test_expect_success 'grep -c counts matches' '
	cd grep-icase &&
	git grep -c -i "hello" >actual &&
	grep "3" actual
'

test_done
