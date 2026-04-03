#!/bin/sh

test_description='diff order & rotate'

. ./test-lib.sh

create_files () {
	echo "$1" >a.h &&
	echo "$1" >b.c &&
	echo "$1" >c/Makefile &&
	echo "$1" >d.txt &&
	git add a.h b.c c/Makefile d.txt &&
	git commit -m "$1"
}

test_expect_success 'setup' '
	git init &&
	git config user.email test@test.com &&
	git config user.name "Test User" &&
	mkdir c &&
	create_files 1 &&
	create_files 2 &&
	cat >expect_none <<-\EOF
	a.h
	b.c
	c/Makefile
	d.txt
	EOF
'

test_expect_success 'no order (=tree object order)' '
	git diff --name-only HEAD~1 HEAD >actual &&
	test_cmp expect_none actual
'

test_expect_failure 'orderfile using option (-O) (not implemented)' '
	cat >order_file_1 <<-\EOF &&
	*Makefile
	*.txt
	*.h
	EOF
	git diff -Oorder_file_1 --name-only HEAD~1 HEAD >actual &&
	cat >expect_1 <<-\EOF &&
	c/Makefile
	d.txt
	a.h
	b.c
	EOF
	test_cmp expect_1 actual
'

test_expect_success 'diff --rotate-to' '
	git diff --rotate-to=b.c --name-only HEAD~1 HEAD >actual
'

test_expect_success 'diff --skip-to' '
	git diff --skip-to=b.c --name-only HEAD~1 HEAD >actual
'

test_done
