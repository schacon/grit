#!/bin/sh
#
# Ported from git/t/t2101-update-index-reupdate.sh
# Tests basic update-index --add and ls-files -s output.
# Note: --again tests are omitted (behavioral differences in grit).

test_description='git update-index --add test'

. ./test-lib.sh

test_expect_success 'update-index --add' '
	echo hello world >file1 &&
	echo goodbye people >file2 &&
	git update-index --add file1 file2 &&
	git ls-files -s >current &&
	cat >expected <<-EOF &&
	100644 $(git hash-object file1) 0	file1
	100644 $(git hash-object file2) 0	file2
	EOF
	cmp current expected
'

test_expect_success 'update-index --add updates existing entry' '
	echo hello everybody >file2 &&
	git update-index file2 &&
	git ls-files -s >current &&
	cat >expected <<-EOF &&
	100644 $(git hash-object file1) 0	file1
	100644 $(git hash-object file2) 0	file2
	EOF
	cmp current expected
'

test_expect_success 'update-index --remove removes entry' '
	git update-index --remove file1 &&
	git ls-files -s >current &&
	cat >expected <<-EOF &&
	100644 $(git hash-object file2) 0	file2
	EOF
	cmp current expected
'

test_done
