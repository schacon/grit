#!/bin/sh

test_description='multi-pack bitmaps with rev operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit A &&
	test_commit B
'

test_expect_success 'pack-objects --all produces valid pack' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'verify-pack -v shows expected objects' '
	git verify-pack -v testpack-*.pack >output &&
	test_grep "commit" output &&
	test_grep "blob" output &&
	test_grep "tree" output
'

test_done
