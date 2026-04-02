#!/bin/sh
# Tests for ref^{stuff}, ported from git/t/t1511-rev-parse-caret.sh.

test_description='grit rev-parse caret and tilde navigation'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with blob-tag, tree-tag, commit-tag, and branches' '
	grit init repo &&
	cd repo &&
	grit config user.email "test@test.com" &&
	grit config user.name "Test" &&

	echo blob >a-blob &&
	blob_oid=$(grit hash-object -w a-blob) &&

	cat >blob-tag-obj <<-EOF &&
	object $blob_oid
	type blob
	tag blob-tag
	tagger grit <grit@example.com> 0 +0000

	blob tag
	EOF
	blob_tag_oid=$(grit hash-object -t tag -w blob-tag-obj) &&
	grit update-ref refs/tags/blob-tag "$blob_tag_oid" &&

	mkdir a-tree &&
	echo moreblobs >a-tree/another-blob &&
	grit add . &&
	TREE_SHA1=$(grit write-tree) &&

	cat >tree-tag-obj <<-EOF &&
	object $TREE_SHA1
	type tree
	tag tree-tag
	tagger grit <grit@example.com> 0 +0000

	tree tag
	EOF
	tree_tag_oid=$(grit hash-object -t tag -w tree-tag-obj) &&
	grit update-ref refs/tags/tree-tag "$tree_tag_oid" &&

	grit commit -m Initial &&
	initial_commit=$(grit rev-parse HEAD) &&

	cat >commit-tag-obj <<-EOF &&
	object $initial_commit
	type commit
	tag commit-tag
	tagger grit <grit@example.com> 0 +0000

	commit tag
	EOF
	commit_tag_oid=$(grit hash-object -t tag -w commit-tag-obj) &&
	grit update-ref refs/tags/commit-tag "$commit_tag_oid" &&

	grit branch ref &&
	echo modified >>a-blob &&
	grit add -u &&
	grit commit -m Modified &&
	grit branch modref &&
	echo "changed!" >>a-blob &&
	grit add -u &&
	grit commit -m "!Exp" &&
	grit branch expref &&
	echo changed >>a-blob &&
	grit add -u &&
	grit commit -m Changed &&
	echo changed-again >>a-blob &&
	grit add -u &&
	grit commit -m Changed-again &&

	echo "$TREE_SHA1" >tree_sha1.out &&
	echo "$initial_commit" >initial_commit.out
'

test_expect_success 'ref^{non-existent}' '
	cd repo &&
	test_must_fail grit rev-parse ref^{non-existent}
'

test_expect_success 'ref^{} peels to commit' '
	cd repo &&
	grit rev-parse ref >expected &&
	grit rev-parse ref^{} >actual &&
	test_cmp expected actual
'

test_expect_success 'commit-tag^{} peels to tagged commit' '
	cd repo &&
	grit rev-parse ref >expected &&
	grit rev-parse commit-tag^{} >actual &&
	test_cmp expected actual
'

test_expect_success 'ref^{commit} resolves ref to commit' '
	cd repo &&
	grit rev-parse ref >expected &&
	grit rev-parse ref^{commit} >actual &&
	test_cmp expected actual
'

test_expect_success 'commit-tag^{commit} peels to commit' '
	cd repo &&
	grit rev-parse ref >expected &&
	grit rev-parse commit-tag^{commit} >actual &&
	test_cmp expected actual
'

test_expect_success 'tree-tag^{commit} fails' '
	cd repo &&
	test_must_fail grit rev-parse tree-tag^{commit}
'

test_expect_success 'blob-tag^{commit} fails' '
	cd repo &&
	test_must_fail grit rev-parse blob-tag^{commit}
'

test_expect_success 'ref^{tree} resolves to tree of ref commit' '
	cd repo &&
	TREE_SHA1=$(cat tree_sha1.out) &&
	echo "$TREE_SHA1" >expected &&
	grit rev-parse ref^{tree} >actual &&
	test_cmp expected actual
'

test_expect_success 'commit-tag^{tree} peels to tree of tagged commit' '
	cd repo &&
	TREE_SHA1=$(cat tree_sha1.out) &&
	echo "$TREE_SHA1" >expected &&
	grit rev-parse commit-tag^{tree} >actual &&
	test_cmp expected actual
'

test_expect_success 'tree-tag^{tree} resolves to tagged tree' '
	cd repo &&
	TREE_SHA1=$(cat tree_sha1.out) &&
	echo "$TREE_SHA1" >expected &&
	grit rev-parse tree-tag^{tree} >actual &&
	test_cmp expected actual
'

test_expect_success 'blob-tag^{tree} fails' '
	cd repo &&
	test_must_fail grit rev-parse blob-tag^{tree}
'

test_expect_success 'HEAD~2 resolves to grandparent' '
	cd repo &&
	initial=$(cat initial_commit.out) &&
	grit rev-parse HEAD~4 >actual &&
	echo "$initial" >expected &&
	test_cmp expected actual
'

test_expect_success 'HEAD^1^1 equals HEAD~2' '
	cd repo &&
	grit rev-parse HEAD~2 >expected &&
	grit rev-parse HEAD^1^1 >actual &&
	test_cmp expected actual
'

test_expect_success 'navigating past initial commit fails' '
	cd repo &&
	test_must_fail grit rev-parse HEAD~100
'

test_expect_success 'HEAD^0 equals HEAD' '
	cd repo &&
	grit rev-parse HEAD >expected &&
	grit rev-parse HEAD^0 >actual &&
	test_cmp expected actual
'

test_expect_success 'commit^{} is idempotent' '
	cd repo &&
	grit rev-parse HEAD >expected &&
	grit rev-parse HEAD^{} >actual &&
	test_cmp expected actual
'

# --- Additional caret/peel tests ---

test_expect_success 'blob-tag^{tree} fails' '
	cd repo &&
	test_must_fail grit rev-parse blob-tag^{tree}
'

test_expect_success 'commit-tag^{} double peel is idempotent' '
	cd repo &&
	grit rev-parse commit-tag^{} >first &&
	result=$(cat first) &&
	grit rev-parse "$result^{}" >second &&
	test_cmp first second
'

test_expect_success 'tree-tag^{} peels to tree' '
	cd repo &&
	TREE_SHA1=$(cat tree_sha1.out) &&
	echo "$TREE_SHA1" >expected &&
	grit rev-parse tree-tag^{} >actual &&
	test_cmp expected actual
'

test_expect_success 'commit-tag^{commit} equals ref' '
	cd repo &&
	grit rev-parse ref >expected &&
	grit rev-parse commit-tag^{commit} >actual &&
	test_cmp expected actual
'

test_expect_success 'ref^{tree} resolves HEAD to tree' '
	cd repo &&
	grit rev-parse HEAD^{tree} >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success 'ref^{commit}^{tree} peels commit then to tree' '
	cd repo &&
	grit rev-parse ref^{tree} >expected &&
	grit rev-parse ref^{commit} >commit_oid &&
	commit=$(cat commit_oid) &&
	grit rev-parse "$commit^{tree}" >actual &&
	test_cmp expected actual
'

test_expect_success 'tree-tag^{blob} fails' '
	cd repo &&
	test_must_fail grit rev-parse tree-tag^{blob}
'

test_expect_success 'HEAD^1^{commit} same as HEAD^1' '
	cd repo &&
	grit rev-parse HEAD^1 >expected &&
	grit rev-parse HEAD^1^{commit} >actual &&
	test_cmp expected actual
'

test_expect_success 'HEAD^1^{tree} gives parent tree' '
	cd repo &&
	grit rev-parse HEAD^1^{tree} >actual &&
	grit rev-parse HEAD^{tree} >head_tree &&
	! test_cmp head_tree actual
'

test_expect_success 'HEAD~3^{commit} resolves' '
	cd repo &&
	grit rev-parse HEAD~3 >expected &&
	grit rev-parse HEAD~3^{commit} >actual &&
	test_cmp expected actual
'

test_expect_success 'HEAD~3^{tree} resolves to tree' '
	cd repo &&
	grit rev-parse HEAD~3^{tree} >actual &&
	hash=$(cat actual) &&
	test $(echo "$hash" | wc -c) = 41
'

test_expect_success 'bad type in ^{badtype} fails' '
	cd repo &&
	test_must_fail grit rev-parse HEAD^{foobar}
'

test_expect_success 'commit-tag^{tree} != tree-tag^{tree} check objects differ' '
	cd repo &&
	grit rev-parse commit-tag^{tree} >commit_tree &&
	grit rev-parse tree-tag^{tree} >tree_tree &&
	test_cmp commit_tree tree_tree
'

test_done
