#!/bin/sh

test_description='fsck on buffers without NUL termination

The goal here is to make sure that the various fsck parsers never look
past the end of the buffer they are given, even when encountering broken
or truncated objects.
'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git commit --allow-empty -m foo &&
	git rev-parse --verify HEAD >.commit_oid &&
	git rev-parse --verify HEAD^{tree} >.tree_oid
'

# Test truncated commit objects via hash-object -t commit

test_expect_success 'truncated commit: empty' '
	echo "" >input &&
	test_must_fail git hash-object -t commit input
'

test_expect_success 'truncated commit: partial tree keyword' '
	echo "tr" >input &&
	test_must_fail git hash-object -t commit input
'

test_expect_success 'truncated commit: tree keyword only' '
	echo "tree" >input &&
	test_must_fail git hash-object -t commit input
'

test_expect_success 'truncated commit: tree with space but no sha' '
	echo "tree " >input &&
	test_must_fail git hash-object -t commit input
'

test_expect_success 'truncated commit: tree with partial sha' '
	echo "tree 1234" >input &&
	test_must_fail git hash-object -t commit input
'

test_expect_success 'truncated commit: tree ok but missing author' '
	tree=$(cat .tree_oid) &&
	echo "tree $tree" >input &&
	test_must_fail git hash-object -t commit input
'

test_expect_success 'truncated commit: tree+parent ok but missing author' '
	tree=$(cat .tree_oid) &&
	commit=$(cat .commit_oid) &&
	printf "tree %s\nparent %s\n" "$tree" "$commit" >input &&
	test_must_fail git hash-object -t commit input
'

test_expect_success 'truncated commit: partial author' '
	tree=$(cat .tree_oid) &&
	printf "tree %s\nauthor\n" "$tree" >input &&
	test_must_fail git hash-object -t commit input
'

test_expect_success 'truncated commit: author ok but missing committer' '
	tree=$(cat .tree_oid) &&
	printf "tree %s\nauthor name <email> 1234 +0000\n" "$tree" >input &&
	test_must_fail git hash-object -t commit input
'

test_expect_success 'truncated commit: partial committer' '
	tree=$(cat .tree_oid) &&
	printf "tree %s\nauthor name <email> 1234 +0000\ncommitter\n" "$tree" >input &&
	test_must_fail git hash-object -t commit input
'

# Test truncated tag objects
test_expect_success 'truncated tag: empty' '
	echo "" >input &&
	test_must_fail git hash-object -t tag input
'

test_expect_success 'truncated tag: partial object keyword' '
	echo "obj" >input &&
	test_must_fail git hash-object -t tag input
'

test_expect_success 'truncated tag: object keyword only' '
	echo "object" >input &&
	test_must_fail git hash-object -t tag input
'

test_expect_success 'truncated tag: object with space but no sha' '
	echo "object " >input &&
	test_must_fail git hash-object -t tag input
'

test_expect_success 'truncated tag: object ok but missing type' '
	commit=$(cat .commit_oid) &&
	echo "object $commit" >input &&
	test_must_fail git hash-object -t tag input
'

test_expect_success 'truncated tag: object+type ok but missing tag name' '
	commit=$(cat .commit_oid) &&
	printf "object %s\ntype commit\n" "$commit" >input &&
	test_must_fail git hash-object -t tag input
'

test_expect_success 'truncated tag: object+type+tag ok but missing tagger' '
	commit=$(cat .commit_oid) &&
	printf "object %s\ntype commit\ntag foo\n" "$commit" >input &&
	test_must_fail git hash-object -t tag input
'

# Test truncated tree (binary format)
test_expect_failure 'truncated tree: short hash' '
	printf "100644 foo\0\1\1\1\1" >input &&
	test_must_fail git hash-object -t tree input
'

test_expect_failure 'truncated tree: missing nul' '
	printf "100644 a long filename, or a hash with missing nul?" >input &&
	test_must_fail git hash-object -t tree input
'

test_done
