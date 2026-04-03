#!/bin/sh

test_description='pseudo-merge bitmaps'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit base &&
	git repack -ad
'

test_expect_success 'write-bitmap-index works' '
	git repack -ad --write-bitmap-index &&
	ls .git/objects/pack/*.bitmap
'

test_expect_success 'pseudo-merge bitmap config accepted' '
	git -c bitmapPseudoMerge.all.pattern="refs/" \
	    -c bitmapPseudoMerge.all.maxMerges=64 \
	    repack -ad --write-bitmap-index
'

test_done
