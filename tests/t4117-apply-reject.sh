#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#

test_description='git apply with rejects'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	test_write_lines 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 >file1 &&
	cat file1 >saved.file1 &&
	git add file1 &&
	git commit -m initial &&

	test_write_lines 1 2 A B 4 5 6 7 8 9 10 11 12 C 13 14 15 16 17 18 19 20 D 21 >file1 &&
	git diff >patch.1 &&

	cat file1 >clean
'

test_expect_success 'apply should fail on conflicting patch' '
	cat saved.file1 >file1 &&
	test_write_lines 1 E 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 F 21 >file1 &&
	test_must_fail git apply patch.1
'

test_expect_success 'apply cleanly on matching file' '
	cat saved.file1 >file1 &&
	git apply patch.1 &&
	test_cmp file1 clean
'

test_expect_success 'apply --check detects conflicts' '
	cat saved.file1 >file1 &&
	test_write_lines 1 E 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 F 21 >file1 &&
	test_must_fail git apply --check patch.1
'

test_expect_success 'apply --check succeeds on clean file' '
	cat saved.file1 >file1 &&
	git apply --check patch.1
'

test_expect_success 'apply --stat shows stats without applying' '
	git apply --stat patch.1 >stat_output &&
	grep "file1" stat_output &&
	# Verify file was not modified
	cat saved.file1 >file1 &&
	git apply --stat patch.1 &&
	test_cmp file1 saved.file1
'

test_expect_success 'apply --numstat shows machine-readable stats' '
	git apply --numstat patch.1 >numstat_output &&
	grep "file1" numstat_output
'

test_done
