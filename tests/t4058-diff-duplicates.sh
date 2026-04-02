#!/bin/sh
# Tests for diff with duplicate entries in trees.

test_description='grit diff with duplicate tree entries'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo
'

test_expect_success 'mktree allows duplicate entries' '
	cd repo &&
	oid_a=$(echo "content a" | git hash-object -w --stdin) &&
	oid_b=$(echo "content b" | git hash-object -w --stdin) &&
	tree=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	test -n "$tree"
'

test_expect_success 'ls-tree shows both duplicate entries' '
	cd repo &&
	oid_a=$(echo "content a" | git hash-object -w --stdin) &&
	oid_b=$(echo "content b" | git hash-object -w --stdin) &&
	tree=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	git ls-tree $tree >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'diff-tree: normal tree vs duplicate-entry tree' '
	cd repo &&
	oid_a=$(echo "content a" | git hash-object -w --stdin) &&
	oid_b=$(echo "content b" | git hash-object -w --stdin) &&
	tree_norm=$(printf "100644 blob %s\tfile\n" "$oid_a" | git mktree) &&
	tree_dup=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	git diff-tree $tree_norm $tree_dup >actual &&
	test -n "$(cat actual)"
'

test_expect_success 'diff-tree: duplicate tree vs itself is empty' '
	cd repo &&
	oid_a=$(echo "content a" | git hash-object -w --stdin) &&
	oid_b=$(echo "content b" | git hash-object -w --stdin) &&
	tree_dup=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	git diff-tree $tree_dup $tree_dup >actual &&
	test_must_fail test -s actual
'

test_expect_success 'diff-tree -p: normal vs duplicate shows patch' '
	cd repo &&
	oid_a=$(echo "content a" | git hash-object -w --stdin) &&
	oid_b=$(echo "content b" | git hash-object -w --stdin) &&
	tree_norm=$(printf "100644 blob %s\tfile\n" "$oid_a" | git mktree) &&
	tree_dup=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	git diff-tree -p $tree_norm $tree_dup >actual &&
	grep "diff --git" actual
'

test_expect_success 'read-tree loads duplicate entries into index' '
	cd repo &&
	oid_a=$(echo "content a" | git hash-object -w --stdin) &&
	oid_b=$(echo "content b" | git hash-object -w --stdin) &&
	tree_dup=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	git read-tree $tree_dup &&
	git ls-files --stage file >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'cat-file -p shows both entries in duplicate tree' '
	cd repo &&
	oid_a=$(echo "content a" | git hash-object -w --stdin) &&
	oid_b=$(echo "content b" | git hash-object -w --stdin) &&
	tree_dup=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	git cat-file -p $tree_dup >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'diff-tree between two different duplicate trees' '
	cd repo &&
	oid_a=$(echo "content a" | git hash-object -w --stdin) &&
	oid_b=$(echo "content b" | git hash-object -w --stdin) &&
	oid_c=$(echo "content c" | git hash-object -w --stdin) &&
	tree1=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	tree2=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_c" | git mktree) &&
	git diff-tree $tree1 $tree2 >actual &&
	test -n "$(cat actual)"
'

test_expect_success 'duplicate entry tree has valid object type' '
	cd repo &&
	oid_a=$(echo "aa" | git hash-object -w --stdin) &&
	oid_b=$(echo "bb" | git hash-object -w --stdin) &&
	tree=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	echo tree >expect &&
	git cat-file -t $tree >actual &&
	test_cmp expect actual
'

test_expect_success 'duplicate entry tree with different modes' '
	cd repo &&
	oid=$(echo "data" | git hash-object -w --stdin) &&
	tree=$(printf "100644 blob %s\tfile\n100755 blob %s\tfile\n" "$oid" "$oid" | git mktree) &&
	git ls-tree $tree >actual &&
	grep "100644" actual &&
	grep "100755" actual
'

test_expect_success 'tree with three duplicate-name entries' '
	cd repo &&
	oid_a=$(echo "a" | git hash-object -w --stdin) &&
	oid_b=$(echo "b" | git hash-object -w --stdin) &&
	oid_c=$(echo "c" | git hash-object -w --stdin) &&
	tree=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n100644 blob %s\tfile\n" \
		"$oid_a" "$oid_b" "$oid_c" | git mktree) &&
	git ls-tree $tree >actual &&
	test $(wc -l <actual) -eq 3
'

test_expect_success 'diff-tree: empty tree vs duplicate-entry tree' '
	cd repo &&
	empty_tree=$(printf "" | git mktree) &&
	oid_a=$(echo "a" | git hash-object -w --stdin) &&
	oid_b=$(echo "b" | git hash-object -w --stdin) &&
	tree_dup=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	git diff-tree $empty_tree $tree_dup >actual &&
	test -n "$(cat actual)"
'

test_expect_success 'diff-tree: duplicate-entry tree vs empty tree' '
	cd repo &&
	empty_tree=$(printf "" | git mktree) &&
	oid_a=$(echo "a" | git hash-object -w --stdin) &&
	oid_b=$(echo "b" | git hash-object -w --stdin) &&
	tree_dup=$(printf "100644 blob %s\tfile\n100644 blob %s\tfile\n" "$oid_a" "$oid_b" | git mktree) &&
	git diff-tree $tree_dup $empty_tree >actual &&
	test -n "$(cat actual)"
'

test_expect_success 'duplicate entries with mixed blob and tree' '
	cd repo &&
	oid=$(echo "sub" | git hash-object -w --stdin) &&
	subtree=$(printf "100644 blob %s\tx\n" "$oid" | git mktree) &&
	tree=$(printf "100644 blob %s\tname\n040000 tree %s\tname\n" "$oid" "$subtree" | git mktree) &&
	git ls-tree $tree >actual &&
	test $(wc -l <actual) -eq 2
'

test_expect_success 'normal tree with unique entries for comparison' '
	cd repo &&
	oid_a=$(echo "ua" | git hash-object -w --stdin) &&
	oid_b=$(echo "ub" | git hash-object -w --stdin) &&
	tree=$(printf "100644 blob %s\tfile_a\n100644 blob %s\tfile_b\n" "$oid_a" "$oid_b" | git mktree) &&
	git ls-tree $tree >actual &&
	test $(wc -l <actual) -eq 2 &&
	grep "file_a" actual &&
	grep "file_b" actual
'

test_done
