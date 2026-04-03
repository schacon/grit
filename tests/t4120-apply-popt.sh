#!/bin/sh
#
# Copyright (c) 2007 Shawn O. Pearce
#

test_description='git apply -p handling.'

. ./test-lib.sh

test_expect_success setup '
	mkdir sub &&
	echo A >sub/file1 &&
	cp sub/file1 file1.saved &&
	git add sub/file1 &&
	echo B >sub/file1 &&
	git diff >patch.file &&
	git checkout -- sub/file1
'

test_expect_success 'apply git diff with -p2' '
	cp file1.saved file1 &&
	git apply -p2 patch.file
'

test_done
