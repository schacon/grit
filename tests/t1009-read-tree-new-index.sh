#!/bin/sh
# Ported subset from git/t/t1009-read-tree-new-index.sh.

test_description='grit read-tree with fresh index file'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup commit referenced by main' '
	grit init repo &&
	cd repo &&
	echo one >a &&
	grit update-index --add a &&
	tree=$(grit write-tree) &&
	commit=$(echo initial | grit commit-tree "$tree") &&
	grit update-ref refs/heads/main "$commit"
'

test_expect_success 'non-existent GIT_INDEX_FILE is created by read-tree' '
	cd repo &&
	rm -f new-index &&
	GIT_INDEX_FILE=new-index grit read-tree main &&
	test_path_is_file new-index
'

test_expect_success 'empty GIT_INDEX_FILE is replaced by read-tree' '
	cd repo &&
	rm -f new-index &&
	>new-index &&
	GIT_INDEX_FILE=new-index grit read-tree main &&
	test_path_is_file new-index
'

test_done
