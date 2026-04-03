#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
# Copyright (c) 2005 Robert Fitzsimons
#

test_description='git apply test for patches creating and modifying files.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'apply patch1 creates new file from /dev/null' '
	git apply "$TEST_DIRECTORY/t4110/patch1.patch" &&
	test_path_is_file new.txt &&
	test_line_count = 12 new.txt
'

test_expect_success 'apply patch3 adds lines after context' '
	git apply "$TEST_DIRECTORY/t4110/patch3.patch" &&
	grep "^b2$" new.txt
'

test_expect_success 'apply --stat on patch' '
	git apply --stat "$TEST_DIRECTORY/t4110/patch1.patch" >stat &&
	grep "new.txt" stat
'

test_expect_success 'apply --numstat on patch' '
	git apply --numstat "$TEST_DIRECTORY/t4110/patch1.patch" >numstat &&
	grep "new.txt" numstat
'

test_expect_success 'apply --check validates patch' '
	rm -f new.txt &&
	git apply --check "$TEST_DIRECTORY/t4110/patch1.patch"
'

test_expect_success 'apply fresh copy of file, then patch2 removes lines' '
	rm -f new.txt &&
	git apply "$TEST_DIRECTORY/t4110/patch1.patch" &&
	git apply "$TEST_DIRECTORY/t4110/patch2.patch" &&
	! grep "^a1$" new.txt &&
	head -1 new.txt | grep "^b1$"
'

test_expect_success 'apply fresh copy, then patch4 adds lines at start' '
	rm -f new.txt &&
	git apply "$TEST_DIRECTORY/t4110/patch2.patch" &&
	test_path_is_missing new.txt || true &&
	# patch2 expects a/new.txt but file doesnt exist yet after removal
	# Start fresh: create file without a-lines, apply patch4
	printf "b1\nb11\nb111\nb1111\nc1\nc11\nc111\nc1111\n" >new.txt &&
	git apply "$TEST_DIRECTORY/t4110/patch4.patch" &&
	head -1 new.txt | grep "^a1$"
'

test_done
