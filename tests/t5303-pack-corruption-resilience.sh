#!/bin/sh

test_description='resilience to pack corruptions with redundant objects'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "base content for file 1" >file_1 &&
	echo "base content for file 2 with delta" >file_2 &&
	echo "base content for file 3 with delta delta" >file_3 &&
	git add file_1 file_2 file_3 &&
	test_tick &&
	git commit -m "initial"
'

test_expect_success 'pack-objects --all packs everything' '
	git pack-objects --all allpack &&
	git verify-pack allpack-*.pack
'

test_expect_success 'verify-pack -v shows all object types' '
	git verify-pack -v allpack-*.pack >output &&
	test_grep "blob" output &&
	test_grep "tree" output &&
	test_grep "commit" output
'

test_expect_success 'pack-objects --stdout produces valid pack' '
	echo HEAD | git pack-objects --revs --stdout >stdout.pack &&
	git index-pack --stdin <stdout.pack
'

test_expect_success 'show-index reads pack index' '
	idx=$(ls allpack-*.idx) &&
	git show-index <"$idx" >output &&
	test_line_count -ge 1 output
'

test_done
