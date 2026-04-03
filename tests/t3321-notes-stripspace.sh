#!/bin/sh

test_description='Test commit notes stripspace behavior'

. ./test-lib.sh

test_expect_success 'setup the commit' '
	git init -q &&
	test_commit 1st
'

test_expect_success 'add note with -m works' '
	git notes add -m "simple note" &&
	echo "simple note" >expect &&
	git notes show >actual &&
	test_cmp expect actual &&
	git notes remove
'

test_expect_success 'add note with multi-line -m' '
	git notes add -m "line1
line2
line3" &&
	cat >expect <<-\EOF &&
	line1
	line2
	line3
	EOF
	git notes show >actual &&
	test_cmp expect actual &&
	git notes remove
'

test_expect_success 'append adds separator between notes' '
	git notes add -m "first" &&
	git notes append -m "second" &&
	cat >expect <<-\EOF &&
	first

	second
	EOF
	git notes show >actual &&
	test_cmp expect actual
'

test_done
