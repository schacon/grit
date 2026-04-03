#!/bin/sh
# Ported from upstream git t7813-grep-icase-iso.sh

test_description='grep icase with various encodings'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init grep-iso &&
	cd grep-iso &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	printf "TILRAUN: Hello World!\n" >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'grep basic case-insensitive' '
	cd grep-iso &&
	git grep -i "hello" >actual &&
	grep "Hello" actual
'

test_expect_success 'grep case-sensitive' '
	cd grep-iso &&
	git grep "TILRAUN" >actual &&
	test $(wc -l <actual) -eq 1
'

test_done
