#!/bin/sh
# Adapted from git/t/t6438-submodule-directory-file-conflicts.sh
# Tests directory/file conflict scenarios

test_description='directory-file conflicts during merge'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: file on one side, directory on another' '
	git init df-conflict &&
	cd df-conflict &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo "base" >base &&
	git add base &&
	git commit -m "initial" &&

	git branch file-side &&
	git branch dir-side &&

	git checkout file-side &&
	echo "I am a file" >path &&
	git add path &&
	git commit -m "add path as file" &&

	git checkout dir-side &&
	mkdir path &&
	echo "I am in a dir" >path/file &&
	git add path/file &&
	git commit -m "add path as directory"
'

test_expect_success 'directory-file conflict during merge detected' '
	cd df-conflict &&
	git checkout file-side &&
	test_must_fail git merge dir-side -m "should conflict"
'

test_done
