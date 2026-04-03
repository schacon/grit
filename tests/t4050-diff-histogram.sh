#!/bin/sh

test_description='histogram diff algorithm

Exercise --histogram via diff --no-index.'

. ./test-lib.sh

# The upstream test sources lib-diff-alternative.sh and calls
# test_diff_frobnitz "histogram" / test_diff_unique "histogram".
# We exercise the most representative cases.

test_expect_success 'histogram diff: simple file comparison' '
	cat >file1 <<-\EOF &&
	line1
	line2
	line3
	EOF
	cat >file2 <<-\EOF &&
	line1
	changed
	line3
	EOF
	test_expect_code 1 git diff --no-index --histogram file1 file2 >out &&
	test -s out
'

test_expect_success 'histogram diff: unique line handling' '
	cat >file1 <<-\EOF &&
	a
	b
	c
	EOF
	cat >file2 <<-\EOF &&
	a
	d
	c
	EOF
	test_expect_code 1 git diff --no-index --histogram file1 file2 >out &&
	test -s out
'

test_done
