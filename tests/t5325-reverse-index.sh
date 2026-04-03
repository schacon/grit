#!/bin/sh

test_description='on-disk reverse index'

. ./test-lib.sh

packdir=.git/objects/pack

test_expect_success 'setup' '
	git init &&
	test_commit base &&
	git pack-objects --all $packdir/pack &&
	test_path_is_missing $packdir/pack-*.rev
'

test_expect_success 'verify-pack works on generated pack' '
	git verify-pack $packdir/pack-*.pack
'

test_expect_success 'verify-pack -v shows object details' '
	git verify-pack -v $packdir/pack-*.pack >output &&
	test_grep "commit" output &&
	test_grep "blob" output &&
	test_grep "tree" output
'

test_expect_success 'show-index works on generated idx' '
	idx=$(ls $packdir/pack-*.idx) &&
	git show-index <"$idx" >output &&
	test_line_count = 3 output
'

test_done
