#!/bin/sh
#
# Copyright (c) 2011 David Caldwell
#

test_description='Test git stash --include-untracked'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'stash --include-untracked saves and cleans untracked files' '
	echo 1 >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	echo 1 >file2 &&
	mkdir -p untracked &&
	echo untracked >untracked/untracked &&
	git stash --include-untracked &&
	test_path_is_missing file2 &&
	test_path_is_missing untracked
'

test_expect_success 'stash pop restores untracked files' '
	git stash pop &&
	test_path_is_file file2 &&
	test_path_is_file untracked/untracked
'

test_expect_success 'stash -u is short for --include-untracked' '
	rm -rf file2 untracked &&
	echo new-content >untracked-file &&
	git stash -u &&
	test_path_is_missing untracked-file &&
	git stash pop &&
	test_path_is_file untracked-file
'

test_expect_success 'stash --include-untracked with dirty tracked file' '
	rm -rf untracked-file &&
	echo changed >file &&
	echo ufile >ufile &&
	git stash --include-untracked &&
	echo 1 >expect &&
	test_cmp expect file &&
	test_path_is_missing ufile &&
	git stash pop &&
	echo changed >expect &&
	test_cmp expect file &&
	test_path_is_file ufile
'

test_done
