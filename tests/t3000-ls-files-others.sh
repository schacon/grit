#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#
# Ported from git/t/t3000-ls-files-others.sh
# Tests that need --directory / --no-empty-directory are skipped (not implemented).

test_description='basic tests for ls-files --others

This test runs git ls-files --others with the following on the
filesystem.

    path0       - a file
    path1	- a symlink
    path2/file2 - a file in a directory
    path3-junk  - a file to confuse things
    path3/file3 - a file in a directory
    path4       - an empty directory
'

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	date >path0 &&
	if test_have_prereq SYMLINKS
	then
		ln -s xyzzy path1
	else
		date >path1
	fi &&
	mkdir path2 path3 path4 &&
	date >path2/file2 &&
	date >path2-junk &&
	date >path3/file3 &&
	date >path3-junk &&
	git update-index --add path3-junk path3/file3
'

test_expect_success 'ls-files --others' '
	cd repo &&
	git ls-files --others >output &&
	cat >expected1 <<-\EOF &&
	output
	path0
	path1
	path2-junk
	path2/file2
	EOF
	test_cmp expected1 output
'

test_expect_failure 'ls-files --others --directory (not implemented)' '
	cd repo &&
	git ls-files --others --directory >output &&
	cat >expected2 <<-\EOF &&
	output
	path0
	path1
	path2-junk
	path2/
	path4/
	EOF
	test_cmp expected2 output
'

test_expect_failure '--no-empty-directory hides empty directory (not implemented)' '
	cd repo &&
	git ls-files --others --directory --no-empty-directory >output &&
	cat >expected3 <<-\EOF &&
	output
	path0
	path1
	path2-junk
	path2/
	EOF
	test_cmp expected3 output
'

test_expect_success 'ls-files --others handles non-submodule .git' '
	cd repo &&
	rm -f expected1 expected2 expected3 &&
	mkdir -p not-a-submodule &&
	echo foo >not-a-submodule/.git &&
	git ls-files -o >output &&
	cat >expected1 <<-\EOF &&
	output
	path0
	path1
	path2-junk
	path2/file2
	EOF
	test_cmp expected1 output
'

test_done
