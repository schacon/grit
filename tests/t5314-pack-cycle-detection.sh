#!/bin/sh

test_description='test handling of inter-pack delta cycles during repack'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "base content" >file &&
	echo one >dummy &&
	git add file dummy &&
	test_tick &&
	git commit -m one &&
	echo "modified content" >file &&
	echo two >dummy &&
	git add file dummy &&
	test_tick &&
	git commit -m two
'

test_expect_success 'pack-objects --all produces valid pack' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'verify-pack -v shows delta info' '
	git verify-pack -v testpack-*.pack >output &&
	test_grep "commit" output &&
	test_grep "blob" output
'

test_expect_success 'cat-file works with loose objects' '
	git cat-file blob HEAD:file >actual &&
	echo "modified content" >expect &&
	test_cmp expect actual
'

test_done
