#!/bin/sh

test_description='test commit-graph with various operations'

. ./test-lib.sh

test_expect_success 'setup: prepare a repository with commits' '
	git init &&
	test_commit the-commit &&
	oid=$(git rev-parse HEAD)
'

test_expect_success 'write commit-graph' '
	git commit-graph write &&
	test_path_is_file .git/objects/info/commit-graph
'

test_expect_success 'verify commit-graph' '
	git commit-graph verify
'

test_expect_success 'rev-list works with commit-graph present' '
	git rev-list HEAD >output &&
	test_line_count = 1 output
'

test_expect_success 'add more commits with graph present' '
	test_commit another &&
	git rev-list HEAD >output &&
	test_line_count = 2 output
'

test_expect_success 'rewrite commit-graph after more commits' '
	git commit-graph write &&
	git commit-graph verify
'

test_done
