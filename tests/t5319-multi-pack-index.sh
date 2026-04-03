#!/bin/sh

test_description='multi-pack-index basic operations'

. ./test-lib.sh

GIT_TEST_MULTI_PACK_INDEX=0
objdir=.git/objects
packdir=$objdir/pack
midx=$packdir/multi-pack-index

test_expect_success 'setup' '
	git init &&
	test_commit base &&
	git repack -d
'

test_expect_success 'write multi-pack-index' '
	git multi-pack-index write &&
	test_path_is_file $midx
'

test_expect_success 'verify multi-pack-index' '
	git multi-pack-index verify
'

test_expect_success 'add objects and rewrite midx' '
	test_commit second &&
	git repack &&
	git multi-pack-index write &&
	test_path_is_file $midx
'

test_expect_success 'verify after update' '
	git multi-pack-index verify
'

test_expect_success 'midx with multiple packs' '
	test_commit third &&
	git repack &&
	test_commit fourth &&
	git repack &&
	git multi-pack-index write &&
	git multi-pack-index verify
'

test_expect_success 'repack -ad removes old packs' '
	git repack -ad &&
	git multi-pack-index write &&
	git multi-pack-index verify
'

test_done
