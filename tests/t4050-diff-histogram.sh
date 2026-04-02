#!/bin/sh

test_description='histogram diff algorithm

Upstream git test t4050 relies on lib-diff-alternative.sh which exercises
--histogram via diff --no-index.  Both --histogram and --no-index are not
yet implemented in grit, so every case is marked test_expect_failure.'

. ./test-lib.sh

# The upstream test sources lib-diff-alternative.sh and calls
# test_diff_frobnitz "histogram" / test_diff_unique "histogram".
# Those helpers exercise diff --no-index --histogram, which grit
# does not support yet.  We stub out the most representative cases
# so the file is ready to flip once the features land.

test_expect_failure 'histogram diff: simple file comparison (not implemented)' '
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
	git diff --no-index --histogram file1 file2
'

test_expect_failure 'histogram diff: unique line handling (not implemented)' '
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
	git diff --no-index --histogram file1 file2
'

test_done
