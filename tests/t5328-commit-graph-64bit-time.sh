#!/bin/sh

test_description='commit-graph with various timestamp scenarios'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit one &&
	test_commit two &&
	test_commit three
'

test_expect_success 'write commit-graph' '
	git commit-graph write &&
	test_path_is_file .git/objects/info/commit-graph
'

test_expect_success 'verify commit-graph' '
	git commit-graph verify
'

test_expect_success 'log works with commit-graph' '
	git log --oneline >output &&
	test_line_count = 3 output
'

test_expect_success 'rev-list works with commit-graph' '
	git rev-list HEAD >output &&
	test_line_count = 3 output
'

test_done
