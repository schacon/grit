#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#

test_description='More rename detection tests.

The rename detection logic should be able to detect pure rename or
copy of symbolic links, but should not produce rename/copy followed
by an edit for them.
'

. ./test-lib.sh

test_expect_success SYMLINKS 'prepare reference tree' '
	git init &&
	echo xyzzy | tr -d '\\'012 >yomin &&
	ln -s xyzzy frotz &&
	git update-index --add frotz yomin &&
	tree=$(git write-tree) &&
	echo $tree &&
	echo $tree >.tree_oid
'

test_expect_success SYMLINKS 'prepare work tree' '
	mv frotz rezrov &&
	rm -f yomin &&
	ln -s xyzzy nitfol &&
	ln -s xzzzy bozbar &&
	git update-index --add --remove frotz rezrov nitfol bozbar yomin
'

test_expect_success SYMLINKS 'setup diff output' '
	tree=$(cat .tree_oid) &&
	GIT_DIFF_OPTS=--unified=0 git diff-index -C -p $tree >current
'

test_expect_success SYMLINKS 'validate diff output' '
	test -s current
'

test_done
