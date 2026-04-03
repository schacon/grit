#!/bin/sh

test_description='multi-pack reuse operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit one &&
	test_commit two &&
	test_commit three
'

test_expect_success 'pack-objects --all creates valid pack' '
	git pack-objects --all testpack &&
	git verify-pack testpack-*.pack
'

test_expect_success 'verify-pack -v shows all commit objects' '
	git verify-pack -v testpack-*.pack >output &&
	grep commit output >commits &&
	test_line_count = 3 commits
'

test_expect_success 'rev-list works' '
	git rev-list HEAD >output &&
	test_line_count = 3 output
'

test_expect_success 'show-index reads generated index' '
	idx=$(ls testpack-*.idx) &&
	git show-index <"$idx" >output &&
	test_line_count -ge 3 output
'

test_done
