#!/bin/sh

test_description='test subject preservation with format-patch | am'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup baseline commit' '
	git init repo && cd repo &&
	echo base >file &&
	git add file &&
	git commit -m "baseline" &&
	git tag baseline
'

test_expect_success 'create patch with short subject' '
	cd repo &&
	git reset --hard baseline &&
	echo short >file &&
	git commit -a -m "short subject" &&
	git format-patch -1 --stdout >short.patch
'

test_expect_success 'short subject preserved (format-patch | am)' '
	cd repo &&
	git reset --hard baseline &&
	git am short.patch &&
	git log -n 1 --format=%s >actual &&
	echo "short subject" >expect &&
	test_cmp expect actual
'

test_expect_success 'create patch with long subject' '
	cd repo &&
	git reset --hard baseline &&
	echo long >file &&
	git commit -a -m "this is a long subject that is virtually guaranteed to require wrapping via format-patch if it is all going to appear on a single line" &&
	git format-patch -1 --stdout >long.patch
'

test_expect_success 'long subject preserved (format-patch | am)' '
	cd repo &&
	git reset --hard baseline &&
	git am long.patch &&
	git log -n 1 --format=%s >actual &&
	echo "this is a long subject that is virtually guaranteed to require wrapping via format-patch if it is all going to appear on a single line" >expect &&
	test_cmp expect actual
'

test_done
