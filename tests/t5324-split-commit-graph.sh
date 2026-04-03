#!/bin/sh

test_description='split commit-graph basic operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	for i in 1 2 3 4 5
	do
		echo "content $i" >file$i &&
		git add file$i &&
		test_tick &&
		git commit -m "commit $i" || return 1
	done
'

test_expect_success 'write commit-graph' '
	git commit-graph write &&
	test_path_is_file .git/objects/info/commit-graph
'

test_expect_success 'verify commit-graph' '
	git commit-graph verify
'

test_expect_success 'add more commits and rewrite graph' '
	for i in 6 7 8
	do
		echo "content $i" >file$i &&
		git add file$i &&
		test_tick &&
		git commit -m "commit $i" || return 1
	done &&
	git commit-graph write
'

test_expect_success 'verify updated graph' '
	git commit-graph verify
'

test_expect_success 'rev-list uses graph correctly' '
	git rev-list HEAD >actual &&
	test_line_count = 8 actual
'

test_done
