#!/bin/sh

test_description='bounds-checking of pack and index file operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "test content" >file &&
	git add file &&
	test_tick &&
	git commit -m "initial"
'

test_expect_success 'pack-objects creates valid pack' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'verify-pack -v shows object info' '
	git verify-pack -v testpack-*.pack >output &&
	test_grep "blob" output
'

test_expect_success 'show-index reads index file' '
	idx=$(ls testpack-*.idx) &&
	git show-index <"$idx" >output &&
	test_line_count -ge 1 output
'

test_expect_success 'cat-file reads loose objects' '
	blob=$(git rev-parse HEAD:file) &&
	git cat-file blob $blob >actual &&
	echo "test content" >expect &&
	test_cmp expect actual
'

test_done
