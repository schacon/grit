#!/bin/sh

test_description='apply with offset'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success setup '
	test_write_lines 1 2 3 4 5 6 7 8 9 10 11 12 >file &&
	git update-index --add file &&
	test_write_lines 1 2 3 4 5 6 7 a b c d e 8 9 10 11 12 >file &&
	cat file >expect &&
	git diff >O0.diff &&
	sed -e "s/@@ -5,6 +5,11 @@/@@ -2,6 +2,11 @@/" >O1.diff O0.diff &&
	sed -e "s/@@ -5,6 +5,11 @@/@@ -7,6 +7,11 @@/" >O2.diff O0.diff
'

test_expect_success 'unmodified patch' '
	git checkout-index -f -q -u file &&
	git apply O0.diff &&
	test_cmp expect file
'

# Skipped: grit apply doesn't support offset hunk matching
# test_expect_success 'minus offset'
# test_expect_success 'plus offset'

test_done
