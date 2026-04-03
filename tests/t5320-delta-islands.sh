#!/bin/sh

test_description='delta islands basic operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "content A" >file &&
	git add file &&
	test_tick &&
	git commit -m A &&
	echo "content B" >file &&
	git commit -a -m B &&
	echo "content C" >file &&
	git commit -a -m C
'

test_expect_success 'pack-objects --all works' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'verify-pack -v shows chain info' '
	git verify-pack -v testpack-*.pack >output &&
	test_grep "chain length" output
'

test_expect_success 'rev-list lists commits' '
	git rev-list HEAD >actual &&
	test_line_count = 3 actual
'

test_done
