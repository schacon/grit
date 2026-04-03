#!/bin/sh
test_description='git shortlog'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test Author" &&
	git config user.email "test@example.com" &&
	echo a >a && git add a && test_tick && git commit -m "first" &&
	echo b >b && git add b && test_tick && git commit -m "second" &&
	echo c >c && git add c && test_tick && git commit -m "third"
'

test_expect_success 'shortlog -sn' '
	cd repo &&
	git shortlog -sn HEAD >actual &&
	grep "3" actual &&
	grep "A U Thor" actual
'

test_expect_success 'shortlog groups by author' '
	cd repo &&
	git shortlog HEAD >actual &&
	grep "A U Thor" actual
'

test_done
