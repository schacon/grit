#!/bin/sh

test_description='multi-pack bitmaps basic operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit one &&
	test_commit two &&
	test_commit three
'

test_expect_success 'repack with bitmaps creates bitmap' '
	git repack -adb &&
	ls .git/objects/pack/pack-*.bitmap >bitmaps &&
	test_line_count = 1 bitmaps
'

test_expect_success 'verify-pack after bitmap repack' '
	git verify-pack .git/objects/pack/pack-*.pack
'

test_expect_success 'write multi-pack-index' '
	git multi-pack-index write &&
	test_path_is_file .git/objects/pack/multi-pack-index
'

test_expect_success 'verify multi-pack-index' '
	git multi-pack-index verify
'

test_done
