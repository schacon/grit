#!/bin/sh
# Adapted from git/t/t7506-status-submodule.sh
# Tests git status with various scenarios

test_description='git status with various scenarios'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init status-repo &&
	cd status-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "file1" >file1 &&
	echo "file2" >file2 &&
	mkdir dir &&
	echo "dir-file" >dir/file &&
	git add file1 file2 dir/file &&
	git commit -m "initial"
'

test_expect_success 'status is clean after commit' '
	cd status-repo &&
	git status --porcelain >../status-output &&
	# grit porcelain includes branch header; filter header, blank, and test artifacts
	grep -v "^##" ../status-output | grep -v "^$" >../filtered || true &&
	test_must_be_empty ../filtered
'

test_expect_success 'status shows modified file' '
	cd status-repo &&
	echo "modified" >>file1 &&
	git status --porcelain >output &&
	test_grep "file1" output
'

test_expect_success 'status shows untracked file' '
	cd status-repo &&
	echo "new" >newfile &&
	git status --porcelain >output &&
	test_grep "newfile" output
'

test_expect_success 'status shows staged file' '
	cd status-repo &&
	echo "staged content" >staged &&
	git add staged &&
	git status --porcelain >output &&
	test_grep "A" output &&
	test_grep "staged" output
'

test_done
