#!/bin/sh
# Ported subset from git/t/t1005-read-tree-reset.sh.

test_description='gust read-tree -u --reset'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup two commits with D/F transition' '
	gust init repo &&
	cd repo &&
	mkdir df &&
	echo content >df/file &&
	gust update-index --add df/file &&
	tree_one=$(gust write-tree) &&
	echo "$tree_one" >../tree_one &&
	commit_one=$(echo one | gust commit-tree "$tree_one") &&
	gust update-ref refs/heads/main "$commit_one" &&
	gust ls-files >expect &&
	rm -rf df &&
	echo content >df &&
	echo content >new &&
	gust update-index --force-remove df/file &&
	gust update-index --add df new &&
	tree_two=$(gust write-tree) &&
	echo "$tree_two" >../tree_two &&
	commit_two=$(echo two | gust commit-tree "$tree_two" -p "$commit_one") &&
	gust update-ref refs/heads/main "$commit_two"
'

test_expect_success 'read-tree -u --reset replaces index with target tree' '
	cd repo &&
	gust read-tree -u --reset "$(cat ../tree_one)" &&
	gust ls-files >actual &&
	test_cmp expect actual
'

test_expect_success 'read-tree --reset -u removes stale working tree files' '
	cd repo &&
	gust read-tree --reset -u "$(cat ../tree_two)" &&
	test_path_is_file new &&
	test_path_is_file df &&
	gust read-tree --reset -u "$(cat ../tree_one)" &&
	test_path_is_missing new &&
	test_path_is_dir df &&
	test_path_is_file df/file
'

test_done
