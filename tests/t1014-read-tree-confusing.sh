#!/bin/sh

test_description='check that read-tree rejects confusing paths'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'create base tree' '
	git init &&
	echo content >file &&
	git add file &&
	git commit -m base &&
	git rev-parse HEAD:file >.blob_oid &&
	git rev-parse HEAD^{tree} >.tree_oid
'

test_expect_success 'enable core.protectHFS for rejection tests' '
	git config core.protectHFS true
'

test_expect_success 'enable core.protectNTFS for rejection tests' '
	git config core.protectNTFS true
'

test_expect_success 'reject . at end of path' '
	blob=$(cat .blob_oid) &&
	printf "100644 blob %s\t.\n" "$blob" >treeinput &&
	bogus=$(git mktree <treeinput) &&
	test_must_fail git read-tree $bogus
'

test_expect_success 'reject .. at end of path' '
	blob=$(cat .blob_oid) &&
	printf "100644 blob %s\t..\n" "$blob" >treeinput &&
	bogus=$(git mktree <treeinput) &&
	test_must_fail git read-tree $bogus
'

test_expect_success 'reject .git at end of path' '
	blob=$(cat .blob_oid) &&
	printf "100644 blob %s\t.git\n" "$blob" >treeinput &&
	bogus=$(git mktree <treeinput) &&
	test_must_fail git read-tree $bogus
'

test_expect_success 'reject .GIT at end of path' '
	blob=$(cat .blob_oid) &&
	printf "100644 blob %s\t.GIT\n" "$blob" >treeinput &&
	bogus=$(git mktree <treeinput) &&
	test_must_fail git read-tree $bogus
'

test_expect_success 'reject git~1 at end of path' '
	blob=$(cat .blob_oid) &&
	printf "100644 blob %s\tgit~1\n" "$blob" >treeinput &&
	bogus=$(git mktree <treeinput) &&
	test_must_fail git read-tree $bogus
'

test_expect_success 'reject . as subtree' '
	tree=$(cat .tree_oid) &&
	printf "040000 tree %s\t.\n" "$tree" >treeinput &&
	bogus=$(git mktree <treeinput) &&
	test_must_fail git read-tree $bogus
'

test_expect_success 'reject .. as subtree' '
	tree=$(cat .tree_oid) &&
	printf "040000 tree %s\t..\n" "$tree" >treeinput &&
	bogus=$(git mktree <treeinput) &&
	test_must_fail git read-tree $bogus
'

test_expect_success 'reject .git as subtree' '
	tree=$(cat .tree_oid) &&
	printf "040000 tree %s\t.git\n" "$tree" >treeinput &&
	bogus=$(git mktree <treeinput) &&
	test_must_fail git read-tree $bogus
'

test_done
