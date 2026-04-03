#!/bin/sh

test_description='check bitmap and pack operations'

. ./test-lib.sh

test_expect_success 'setup repo with history' '
	git init &&
	echo 1 >file &&
	git add file &&
	test_tick &&
	git commit -m orig &&
	echo 2 >file &&
	git commit -a -m update &&
	echo 1 >file &&
	git commit -a -m repeat
'

test_expect_success 'repack with bitmaps creates bitmap file' '
	git repack -adb &&
	ls .git/objects/pack/pack-*.bitmap >bitmaps &&
	test_line_count = 1 bitmaps
'

test_expect_success 'verify pack after bitmap repack' '
	git verify-pack .git/objects/pack/pack-*.pack
'

test_expect_success 'pack-objects --all after bitmap repack' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_done
