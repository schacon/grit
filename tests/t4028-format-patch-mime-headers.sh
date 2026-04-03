#!/bin/sh

test_description='format-patch mime headers and extra headers do not conflict'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'create commit' '
	git init repo &&
	cd repo &&
	echo content >file &&
	git add file &&
	git commit -m one &&
	echo more >>file &&
	git add file &&
	git commit -m "two"
'

test_expect_success 'format-patch generates valid patch' '
	cd repo &&
	git format-patch --stdout HEAD~1 >patch &&
	grep "^Subject:" patch &&
	grep "^diff --git" patch
'

test_expect_success 'format-patch has From line' '
	cd repo &&
	git format-patch --stdout HEAD~1 >patch &&
	grep "^From " patch
'

test_done
