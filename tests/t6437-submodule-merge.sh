#!/bin/sh
# Adapted from git/t/t6437-submodule-merge.sh
# Tests merge in repos that have submodule-like structure

test_description='merge with submodule-like directory structure'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: branches with non-overlapping changes' '
	git init merge-dirs &&
	cd merge-dirs &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	mkdir sub &&
	echo "sub content" >sub/file &&
	echo "main content" >main.txt &&
	git add sub/file main.txt &&
	git commit -m "initial with sub dir" &&

	git branch sideA &&
	git branch sideB &&

	git checkout sideA &&
	echo "A change" >a-file &&
	git add a-file &&
	git commit -m "add a-file" &&

	git checkout sideB &&
	echo "B change" >b-file &&
	git add b-file &&
	git commit -m "add b-file"
'

test_expect_success 'merge does not destroy sub directory' '
	cd merge-dirs &&
	git checkout sideA &&
	git merge sideB -m "merge sideB" &&
	test_path_is_file sub/file &&
	test_path_is_file a-file &&
	test_path_is_file b-file
'

test_done
