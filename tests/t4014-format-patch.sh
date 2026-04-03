#!/bin/sh
#
# Copyright (c) 2006 Junio C Hamano
#

test_description='various format-patch tests'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo &&
	cd repo &&
	test_write_lines 1 2 3 4 5 6 7 8 9 10 >file &&
	git add file &&
	test_tick &&
	git commit -m Initial &&
	git checkout -b side &&

	test_write_lines 1 2 5 6 A B C 7 8 9 10 >file &&
	test_tick &&
	git commit -a -m "Side changes #1" &&

	test_write_lines D E F >>file &&
	git update-index file &&
	test_tick &&
	git commit -m "Side changes #2" &&
	git tag C2 &&

	test_write_lines 5 6 1 2 3 A 4 B C 7 8 9 10 D E F >file &&
	git update-index file &&
	test_tick &&
	git commit -m "Side changes #3 with \\n backslash-n in it."
'

test_expect_success 'format-patch --stdout produces patches' '
	cd repo &&
	git format-patch --stdout HEAD~3 >patch0 &&
	grep "^From " patch0 >from0 &&
	test_line_count = 3 from0
'

test_expect_success 'format-patch result has correct subject' '
	cd repo &&
	git format-patch --stdout HEAD~3 >patch &&
	grep "^Subject: \[PATCH 1/3\] Side changes #1" patch
'

test_expect_success 'format-patch to output directory' '
	cd repo &&
	rm -rf patches/ &&
	git format-patch -o patches/ HEAD~3 &&
	test -f patches/0001-Side-changes--1.patch &&
	test -f patches/0002-Side-changes--2.patch &&
	test -f patches/0003-Side-changes--3-with--n-backslash-n-in-it..patch
'

test_expect_success 'format-patch --subject-prefix' '
	cd repo &&
	git format-patch --stdout --subject-prefix=TESTCASE HEAD~3 >patch-prefix &&
	grep "^Subject: \[TESTCASE 1/3\]" patch-prefix
'

test_expect_success 'format-patch --numbered forces numbering' '
	cd repo &&
	git format-patch --numbered --stdout HEAD~1 >patch-num &&
	grep "^Subject: \[PATCH 1/1\]" patch-num
'

test_expect_success 'format-patch output goes to stdout with --stdout' '
	cd repo &&
	git format-patch --stdout HEAD~3 >stdout-output &&
	test -s stdout-output
'

test_expect_success 'format-patch single patch has no numbering' '
	cd repo &&
	git format-patch --stdout HEAD~1 >single &&
	grep "^Subject: \[PATCH\]" single
'

test_expect_success 'format-patch patch body contains diff' '
	cd repo &&
	git format-patch --stdout HEAD~1 >patch-body &&
	grep "^diff --git" patch-body
'

test_expect_success 'format-patch with A..B range' '
	cd repo &&
	git format-patch --stdout main..side >patch-range &&
	grep "^From " patch-range >from-range &&
	test_line_count = 3 from-range
'

test_expect_success 'format-patch A..B produces correct subjects' '
	cd repo &&
	git format-patch --stdout main..side >patch-range2 &&
	grep "^Subject: \[PATCH 1/3\]" patch-range2 &&
	grep "^Subject: \[PATCH 2/3\]" patch-range2 &&
	grep "^Subject: \[PATCH 3/3\]" patch-range2
'

test_expect_success 'format-patch --root formats from root commit' '
	cd repo &&
	git checkout main &&
	git format-patch --root --stdout HEAD >patch-root &&
	grep "^Subject: \[PATCH\] Initial" patch-root
'

test_expect_success 'format-patch --cover-letter generates cover letter' '
	cd repo &&
	git checkout side &&
	git format-patch --cover-letter --stdout HEAD~2 >patch-cover &&
	grep "^Subject: \[PATCH 0/2\]" patch-cover &&
	grep "^Subject: \[PATCH 1/2\]" patch-cover &&
	grep "^Subject: \[PATCH 2/2\]" patch-cover
'

test_expect_success 'format-patch --no-numbered suppresses numbering' '
	cd repo &&
	git format-patch --no-numbered --stdout HEAD~2 >patch-nonum &&
	cnt=$(grep "^Subject: \[PATCH\]" patch-nonum | wc -l) &&
	test "$cnt" = "2"
'

test_expect_success 'format-patch --start-number adjusts numbering' '
	cd repo &&
	git format-patch --start-number 5 --numbered --stdout HEAD~1 >patch-startnum &&
	grep "^Subject: \[PATCH 5/5\]" patch-startnum
'

test_done
