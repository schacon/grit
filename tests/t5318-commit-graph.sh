#!/bin/sh

test_description='commit graph'

. ./test-lib.sh

test_expect_success 'setup repo with commits' '
	git init &&
	echo "file1" >file1.txt &&
	git add file1.txt &&
	test_tick &&
	git commit -m "first" &&
	echo "file2" >file2.txt &&
	git add file2.txt &&
	test_tick &&
	git commit -m "second" &&
	echo "file3" >file3.txt &&
	git add file3.txt &&
	test_tick &&
	git commit -m "third" &&
	git repack
'

test_expect_success 'write graph' '
	git commit-graph write &&
	test_path_is_file .git/objects/info/commit-graph
'

test_expect_success 'verify written graph' '
	git commit-graph verify
'

test_expect_success 'rev-list works with commit-graph' '
	git rev-list HEAD >actual &&
	test_line_count = 3 actual
'

test_expect_success 'add more commits and rewrite graph' '
	echo "file4" >file4.txt &&
	git add file4.txt &&
	test_tick &&
	git commit -m "fourth" &&
	git repack &&
	git commit-graph write &&
	test_path_is_file .git/objects/info/commit-graph
'

test_expect_success 'verify graph after update' '
	git commit-graph verify
'

test_expect_success 'rev-list reflects new commits' '
	git rev-list HEAD >actual &&
	test_line_count = 4 actual
'

test_expect_success 'log works with commit-graph' '
	git log --oneline >actual &&
	test_line_count = 4 actual
'

test_done
