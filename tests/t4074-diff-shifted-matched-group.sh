#!/bin/sh

test_description='shifted diff groups re-diffing during histogram diff

All tests use --no-index --histogram which grit does not support yet.'

. ./test-lib.sh

test_expect_failure 'shifted/merged diff group should re-diff to minimize patch (not implemented)' '
	test_write_lines A x A A A x A A A >file1 &&
	test_write_lines A x A Z A x A A A >file2 &&
	test_expect_code 1 git diff --no-index --histogram file1 file2 >output &&
	test -s output
'

test_expect_failure 'merged diff group with no shift (not implemented)' '
	test_write_lines A Z B x >file1 &&
	test_write_lines C D x Z E x >file2 &&
	test_expect_code 1 git diff --no-index --histogram file1 file2 >output &&
	test -s output
'

test_expect_failure 're-diff should preserve diff flags (not implemented)' '
	test_write_lines a b c a b c >file1 &&
	test_write_lines x " b" z a b c >file2 &&
	test_expect_code 1 git diff --no-index --histogram file1 file2 >output &&
	test -s output
'

test_expect_failure 'shifting on either side should trigger re-diff properly (not implemented)' '
	test_write_lines a b c a b c a b c >file1 &&
	test_write_lines a b c a1 a2 a3 b c1 a b c >file2 &&
	test_expect_code 1 git diff --no-index --histogram file1 file2 >output &&
	test -s output
'

test_done
