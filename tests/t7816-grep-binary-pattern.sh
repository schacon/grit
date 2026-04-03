#!/bin/sh
# Ported from upstream git t7816-grep-binary-pattern.sh

test_description='grep binary patterns'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init grep-bin &&
	cd grep-bin &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo "hello world" >text.txt &&
	printf "binary\000content\n" >binary.bin &&
	echo "another line" >text2.txt &&
	git add . &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'grep finds text in text files' '
	cd grep-bin &&
	git grep "hello" >actual &&
	grep "text.txt" actual
'

test_expect_success 'grep -l lists matching files' '
	cd grep-bin &&
	git grep -l "hello" >actual &&
	echo "text.txt" >expected &&
	test_cmp expected actual
'

test_expect_success 'grep across multiple files' '
	cd grep-bin &&
	echo "hello also" >text3.txt &&
	git add text3.txt &&
	git commit -m "add text3" &&
	git grep "hello" >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'grep in committed tree' '
	cd grep-bin &&
	git grep "hello" HEAD >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'grep --count' '
	cd grep-bin &&
	git grep -c "hello" >actual &&
	test -s actual
'

test_done
