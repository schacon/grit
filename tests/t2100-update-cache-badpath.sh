#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#

test_description='git update-index nonsense-path test.

This test creates the following structure in the cache:

    path0       - a file
    path1       - a file
    path2/file2 - a file in a directory
    path3/file3 - a file in a directory

and tries to git update-index --add the following:

    path0/file0 - a file in a directory
    path1/file1 - a file in a directory
    path2       - a file
    path3       - a file

All of the attempts should fail.
'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	mkdir path2 path3 &&
	date >path0 &&
	date >path1 &&
	date >path2/file2 &&
	date >path3/file3
'

test_expect_success 'git update-index --add to add various paths.' '
	git update-index --add -- path0 path1 path2/file2 path3/file3
'

test_expect_success 'setup conflicting paths' '
	rm -fr path0 path1 path2 path3 &&
	mkdir path0 path1 &&
	date >path2 &&
	date >path3 &&
	date >path0/file0 &&
	date >path1/file1
'

# grit does not yet detect file/directory conflicts in update-index
for p in path0/file0 path1/file1 path2 path3
do
	test_expect_failure \
	    "git update-index to add conflicting path $p should fail." \
	    "test_must_fail git update-index --add -- $p"
done
test_done
