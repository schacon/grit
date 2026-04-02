#!/bin/sh
# Ported subset from git/t/t1005-read-tree-reset.sh.

test_description='grit read-tree -u --reset'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup two commits with D/F transition' '
	grit init repo &&
	cd repo &&
	mkdir df &&
	echo content >df/file &&
	grit update-index --add df/file &&
	tree_one=$(grit write-tree) &&
	echo "$tree_one" >../tree_one &&
	commit_one=$(echo one | grit commit-tree "$tree_one") &&
	grit update-ref refs/heads/main "$commit_one" &&
	grit ls-files >expect &&
	rm -rf df &&
	echo content >df &&
	echo content >new &&
	grit update-index --force-remove df/file &&
	grit update-index --add df new &&
	tree_two=$(grit write-tree) &&
	echo "$tree_two" >../tree_two &&
	commit_two=$(echo two | grit commit-tree "$tree_two" -p "$commit_one") &&
	grit update-ref refs/heads/main "$commit_two"
'

test_expect_success 'read-tree -u --reset replaces index with target tree' '
	cd repo &&
	grit read-tree -u --reset "$(cat ../tree_one)" &&
	grit ls-files >actual &&
	test_cmp expect actual
'

test_expect_success 'read-tree --reset -u removes stale working tree files' '
	cd repo &&
	grit read-tree --reset -u "$(cat ../tree_two)" &&
	test_path_is_file new &&
	test_path_is_file df &&
	grit read-tree --reset -u "$(cat ../tree_one)" &&
	test_path_is_missing new &&
	test_path_is_dir df &&
	test_path_is_file df/file
'


test_expect_success 'read-tree --reset switches between trees correctly' '
	cd repo &&
	grit read-tree --reset -u "$(cat ../tree_one)" &&
	grit ls-files >after_one &&
	echo "df/file" >expect_one &&
	test_cmp expect_one after_one &&
	grit read-tree --reset -u "$(cat ../tree_two)" &&
	grit ls-files >after_two &&
	printf "df\nnew\n" >expect_two &&
	test_cmp expect_two after_two
'

test_expect_success 'read-tree --reset without -u updates index but leaves working tree' '
	cd repo &&
	grit read-tree --reset -u "$(cat ../tree_two)" &&
	test_path_is_file new &&
	grit read-tree --reset "$(cat ../tree_one)" &&
	grit ls-files >after_idx &&
	echo "df/file" >expected_idx &&
	test_cmp expected_idx after_idx &&
	test_path_is_file new
'


test_done
