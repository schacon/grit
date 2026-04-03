#!/bin/sh

test_description='git conflicts when checking files out test.'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'git update-index --add various paths' '
	date >path0 &&
	mkdir path1 &&
	date >path1/file1 &&
	git update-index --add path0 path1/file1
'

test_expect_success 'setup conflicting work tree' '
	rm -fr path0 path1 &&
	mkdir path0 &&
	date >path0/file0 &&
	date >path1
'

test_expect_success 'git checkout-index without -f should fail on conflicting work tree' '
	test_must_fail git checkout-index -a
'

test_expect_success 'write-tree after update-index' '
	tree1=$(git write-tree) &&
	test -n "$tree1"
'

test_expect_success 'update-index more paths' '
	rm -fr path0 path1 &&
	mkdir path2 path3 &&
	date >path2/file0 &&
	date >path3/file1 &&
	git update-index --add path2/file0 path3/file1 &&
	tree2=$(git write-tree) &&
	test -n "$tree2"
'

test_done
