#!/bin/sh
# Tests for caret/tilde navigation and peel operators, ported from git/t/t1511-rev-parse-caret.sh.

test_description='grit rev-parse caret and tilde navigation'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with chain of commits and tags' '
	grit init repo &&
	cd repo &&
	echo "ref: refs/heads/main" >.git/HEAD &&
	echo one >a &&
	grit hash-object -w a >/dev/null &&
	grit update-index --add a &&
	tree1=$(grit write-tree) &&
	commit1=$(printf "Initial\n" | grit commit-tree "$tree1") &&
	grit update-ref refs/heads/main "$commit1" &&
	echo two >>a &&
	grit hash-object -w a >/dev/null &&
	grit update-index --add a &&
	tree2=$(grit write-tree) &&
	commit2=$(printf "Second\n" | grit commit-tree "$tree2" -p "$commit1") &&
	grit update-ref refs/heads/main "$commit2" &&
	echo three >>a &&
	grit hash-object -w a >/dev/null &&
	grit update-index --add a &&
	tree3=$(grit write-tree) &&
	commit3=$(printf "Third\n" | grit commit-tree "$tree3" -p "$commit2") &&
	grit update-ref refs/heads/main "$commit3" &&
	cat >commit_tag.txt <<-EOF &&
	object $commit3
	type commit
	tag v1
	tagger grit <grit@example.com> 0 +0000

	annotated commit tag
	EOF
	tag_oid=$(grit hash-object -t tag -w commit_tag.txt) &&
	grit update-ref refs/tags/v1 "$tag_oid" &&
	cat >tree_tag.txt <<-EOF &&
	object $tree3
	type tree
	tag tree-v1
	tagger grit <grit@example.com> 0 +0000

	annotated tree tag
	EOF
	tree_tag_oid=$(grit hash-object -t tag -w tree_tag.txt) &&
	grit update-ref refs/tags/tree-v1 "$tree_tag_oid" &&
	echo "$commit1" >commit1.out &&
	echo "$commit2" >commit2.out &&
	echo "$commit3" >commit3.out &&
	echo "$tree3" >tree3.out
'

test_expect_success 'commit^{} returns same commit' '
	cd repo &&
	commit3=$(cat commit3.out) &&
	echo "$commit3" >expect &&
	grit rev-parse HEAD^{} >actual &&
	test_cmp expect actual
'

test_expect_success 'annotated tag^{} peels to tagged commit' '
	cd repo &&
	commit3=$(cat commit3.out) &&
	echo "$commit3" >expect &&
	grit rev-parse v1^{} >actual &&
	test_cmp expect actual
'

test_expect_success 'annotated tag^{commit} peels to tagged commit' '
	cd repo &&
	commit3=$(cat commit3.out) &&
	echo "$commit3" >expect &&
	grit rev-parse v1^{commit} >actual &&
	test_cmp expect actual
'

test_expect_success 'tree-pointing tag^{commit} fails' '
	cd repo &&
	test_must_fail grit rev-parse tree-v1^{commit}
'

test_expect_success 'commit^{tree} resolves to tree object' '
	cd repo &&
	tree3=$(cat tree3.out) &&
	echo "$tree3" >expect &&
	grit rev-parse HEAD^{tree} >actual &&
	test_cmp expect actual
'

test_expect_success 'annotated commit tag^{tree} resolves to tree object' '
	cd repo &&
	tree3=$(cat tree3.out) &&
	echo "$tree3" >expect &&
	grit rev-parse v1^{tree} >actual &&
	test_cmp expect actual
'

test_expect_success 'HEAD~2 resolves to grandparent' '
	cd repo &&
	commit1=$(cat commit1.out) &&
	echo "$commit1" >expect &&
	grit rev-parse HEAD~2 >actual &&
	test_cmp expect actual
'

test_expect_success 'HEAD^1^1 equals HEAD~2' '
	cd repo &&
	grit rev-parse HEAD~2 >expect &&
	grit rev-parse HEAD^1^1 >actual &&
	test_cmp expect actual
'

test_expect_success 'navigating past initial commit fails' '
	cd repo &&
	test_must_fail grit rev-parse HEAD~3
'

test_expect_success '^{non-existent} type fails' '
	cd repo &&
	test_must_fail grit rev-parse HEAD^{non-existent}
'

test_done
