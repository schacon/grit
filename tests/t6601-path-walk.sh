#!/bin/sh

test_description='path-based operations with various tree structures'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	mkdir -p a/b/c &&
	echo file1 >a/file1 &&
	echo file2 >a/b/file2 &&
	echo file3 >a/b/c/file3 &&
	echo root >root &&
	git add a root &&
	git commit -m "initial" &&
	git tag initial
'

test_expect_success 'ls-tree shows all files recursively' '
	git ls-tree -r HEAD >actual &&
	test_line_count = 4 actual &&
	grep "a/file1" actual &&
	grep "a/b/file2" actual &&
	grep "a/b/c/file3" actual &&
	grep "root" actual
'

test_expect_success 'ls-tree shows top-level entries' '
	git ls-tree HEAD >actual &&
	test_line_count = 2 actual &&
	grep "tree" actual | grep "a$" &&
	grep "blob" actual | grep "root$"
'

test_expect_success 'diff-tree shows changes between commits' '
	echo modified >root &&
	git add root &&
	git commit -m "modify root" &&
	git diff-tree --name-only HEAD^ HEAD >actual &&
	grep "root" actual
'

test_expect_success 'diff-tree -r with two trees' '
	tree1=$(git rev-parse initial^{tree}) &&
	tree2=$(git rev-parse HEAD^{tree}) &&
	git diff-tree -r $tree1 $tree2 >actual &&
	grep "root" actual
'

test_expect_success 'ls-tree with multiple levels of nesting' '
	mkdir -p deep/path/to/files &&
	echo deep >deep/path/to/files/content &&
	git add deep &&
	git commit -m "add deep path" &&
	git ls-tree -r HEAD >actual &&
	grep "deep/path/to/files/content" actual
'

test_expect_success 'ls-tree --name-only' '
	git ls-tree --name-only HEAD >actual &&
	grep "^a$" actual &&
	grep "^root$" actual &&
	grep "^deep$" actual
'

test_done
