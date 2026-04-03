#!/bin/sh

test_description='shifted diff groups re-diffing during histogram diff

Tests use --no-index --histogram; grit accepts --histogram as a no-op
(ignores algorithm flag) and uses its default diff algorithm.'

. ./test-lib.sh

test_expect_success 'shifted/merged diff group should re-diff to minimize patch' '
	test_write_lines A x A A A x A A A >file1 &&
	test_write_lines A x A Z A x A A A >file2 &&
	test_expect_code 1 git diff --no-index --histogram file1 file2 >output &&
	test -s output
'

test_expect_success 'merged diff group with no shift' '
	test_write_lines A Z B x >file1 &&
	test_write_lines C D x Z E x >file2 &&
	test_expect_code 1 git diff --no-index --histogram file1 file2 >output &&
	test -s output
'

test_expect_success 're-diff should preserve diff flags' '
	test_write_lines a b c a b c >file1 &&
	test_write_lines x " b" z a b c >file2 &&
	test_expect_code 1 git diff --no-index --histogram file1 file2 >output &&
	test -s output
'

test_expect_success 'shifting on either side should trigger re-diff properly' '
	test_write_lines a b c a b c a b c >file1 &&
	test_write_lines a b c a1 a2 a3 b c1 a b c >file2 &&
	test_expect_code 1 git diff --no-index --histogram file1 file2 >output &&
	test -s output
'

test_done
