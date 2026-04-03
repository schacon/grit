#!/bin/sh

test_description='git pack-objects basic operations'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	for n in 1 2 3 4 5
	do
		echo "This is file: $n" >file.$n &&
		git add file.$n &&
		test_tick &&
		git commit -m "$n" || return 1
	done
'

test_expect_success 'pack-objects --all packs all objects' '
	git pack-objects --all allpack &&
	git verify-pack allpack-*.pack
'

test_expect_success 'verify-pack -v shows blobs, trees, commits' '
	git verify-pack -v allpack-*.pack >output &&
	grep blob output >blob_lines &&
	test_line_count = 5 blob_lines &&
	grep commit output >commit_lines &&
	test_line_count = 5 commit_lines &&
	grep tree output >tree_lines &&
	test_line_count -ge 1 tree_lines
'

test_expect_success 'pack-objects --revs packs reachable objects' '
	echo HEAD | git pack-objects --revs revpack &&
	git verify-pack revpack-*.pack
'

test_expect_success 'show-index lists packed objects' '
	idx=$(ls allpack-*.idx) &&
	git show-index <"$idx" >output &&
	test_line_count -ge 10 output
'

test_done
