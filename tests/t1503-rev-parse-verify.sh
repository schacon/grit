#!/bin/sh
# Ported subset from git/t/t1503-rev-parse-verify.sh.

test_description='grit rev-parse --verify basics'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with commits and tag' '
	grit init repo &&
	cd repo &&
	echo "ref: refs/heads/main" >.git/HEAD &&
	echo one >hello &&
	grit hash-object -w hello >/dev/null &&
	grit update-index --add hello &&
	tree1=$(grit write-tree) &&
	commit1=$(printf "one\n" | grit commit-tree "$tree1") &&
	grit update-ref refs/heads/main "$commit1" &&
	echo two >>hello &&
	grit hash-object -w hello >/dev/null &&
	grit update-index --add hello &&
	tree2=$(grit write-tree) &&
	commit2=$(printf "two\n" | grit commit-tree "$tree2" -p "$commit1") &&
	grit update-ref refs/heads/main "$commit2" &&
	cat >tag.txt <<-EOF &&
	object $commit2
	type commit
	tag v1
	tagger grit <grit@example.com> 0 +0000

	annotated tag
	EOF
	tag_oid=$(grit hash-object -t tag -w tag.txt) &&
	grit update-ref refs/tags/v1 "$tag_oid" &&
	echo "$commit1" >commit1.out &&
	echo "$commit2" >commit2.out &&
	echo "$tree2" >tree2.out &&
	echo "$tag_oid" >tag.out
'

test_expect_success 'verify resolves HEAD and branch name' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify HEAD >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual &&
	grit rev-parse --verify main >actual &&
	test_cmp expect actual
'

test_expect_success 'verify peels annotated tag with ^{} and ^{commit}' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify v1^{} >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual &&
	grit rev-parse --verify v1^{commit} >actual &&
	test_cmp expect actual
'

test_expect_success '--short outputs abbreviated hash; --short=100 saturates' '
	cd repo &&
	grit rev-parse --verify --short HEAD >actual &&
	test "$(wc -c <actual)" -lt 41 &&
	grit rev-parse --verify HEAD >expect &&
	grit rev-parse --verify --short=100 HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'verify fails with bad or multiple revisions' '
	cd repo &&
	test_must_fail grit rev-parse --verify 2>err &&
	grep "single revision" err &&
	test_must_fail grit rev-parse --verify does-not-exist 2>err &&
	grep "single revision" err &&
	test_must_fail grit rev-parse --verify HEAD main 2>err &&
	grep "single revision" err
'

test_expect_success 'verify -q fails silently' '
	cd repo &&
	test_must_fail grit rev-parse --verify -q does-not-exist >out 2>err &&
	test ! -s out &&
	test ! -s err
'

test_expect_success '--default supplies revision when no positional argument' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify --default main >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify respects --end-of-options' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit update-ref refs/heads/-tricky "$commit2" &&
	grit rev-parse --verify HEAD >expect &&
	grit rev-parse --verify --end-of-options -tricky >actual &&
	test_cmp expect actual
'

test_expect_success 'HEAD~1 resolves to first parent commit' '
	cd repo &&
	commit1=$(cat commit1.out) &&
	echo "$commit1" >expect &&
	grit rev-parse HEAD~1 >actual &&
	test_cmp expect actual
'

test_expect_success 'HEAD^1 is same as HEAD~1' '
	cd repo &&
	grit rev-parse HEAD~1 >expect &&
	grit rev-parse HEAD^1 >actual &&
	test_cmp expect actual
'

test_expect_success 'HEAD~2 fails when no grandparent exists' '
	cd repo &&
	test_must_fail grit rev-parse HEAD~2
'

test_expect_success 'verify ^{tree} peels commit to its tree' '
	cd repo &&
	tree2=$(cat tree2.out) &&
	echo "$tree2" >expect &&
	grit rev-parse --verify HEAD^{tree} >actual &&
	test_cmp expect actual
'

test_expect_success 'no stdout output on verify error' '
	cd repo &&
	test_must_fail grit rev-parse --verify >out 2>/dev/null &&
	test_must_be_empty out &&
	test_must_fail grit rev-parse --verify does-not-exist >out 2>/dev/null &&
	test_must_be_empty out
'

test_expect_success '--short=4 outputs minimum 4-character abbreviated hash' '
	cd repo &&
	grit rev-parse --verify --short=4 HEAD >actual &&
	test "$(wc -c <actual)" -ge 5 &&
	test "$(wc -c <actual)" -le 41
'

test_expect_success 'works with one good rev (full hash)' '
	cd repo &&
	commit1=$(cat commit1.out) &&
	commit2=$(cat commit2.out) &&
	rev_hash1=$(grit rev-parse --verify $commit1) &&
	test "$rev_hash1" = "$commit1" &&
	rev_hash2=$(grit rev-parse --verify $commit2) &&
	test "$rev_hash2" = "$commit2"
'

test_expect_success 'fails with baz HEAD (bad before good)' '
	cd repo &&
	test_must_fail grit rev-parse --verify baz HEAD 2>err &&
	grep "single revision" err
'

test_expect_success 'fails with HASH2 HEAD (two good revs)' '
	cd repo &&
	commit1=$(cat commit1.out) &&
	test_must_fail grit rev-parse --verify $commit1 HEAD 2>err &&
	grep "single revision" err
'

test_expect_success 'options can appear after --verify' '
	cd repo &&
	grit rev-parse --verify HEAD >expect &&
	grit rev-parse --verify -q HEAD >actual &&
	test_cmp expect actual
'

test_expect_success '--default with explicit revision uses explicit' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify --default main HEAD >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual
'

test_expect_success '--default without argument uses default' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --default main --verify >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual
'

test_expect_success '--default with bad default fails' '
	cd repo &&
	test_must_fail grit rev-parse --default foo --verify 2>err
'

test_expect_success 'verify --default with bad explicit fails' '
	cd repo &&
	test_must_fail grit rev-parse --verify foo --default main
'

# --- New tests ---

test_expect_success 'verify with full SHA resolves' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify "$commit2" >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify with tag name resolves' '
	cd repo &&
	tag_oid=$(cat tag.out) &&
	grit rev-parse --verify v1 >actual &&
	echo "$tag_oid" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify HEAD^{tree} returns tree hash' '
	cd repo &&
	tree2=$(cat tree2.out) &&
	grit rev-parse --verify "HEAD^{tree}" >actual &&
	echo "$tree2" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify HEAD^0 same as HEAD' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify "HEAD^0" >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify HEAD~0 same as HEAD' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify "HEAD~0" >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify refs/heads/main resolves' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify refs/heads/main >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify refs/tags/v1 resolves to tag object' '
	cd repo &&
	tag_oid=$(cat tag.out) &&
	grit rev-parse --verify refs/tags/v1 >actual &&
	echo "$tag_oid" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify -q with valid ref succeeds silently' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify -q HEAD >actual 2>err &&
	echo "$commit2" >expect &&
	test_cmp expect actual &&
	test_must_be_empty err
'

test_expect_success 'verify tag^{commit} peels annotated tag to commit' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify "v1^{commit}" >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify with --short gives abbreviated output' '
	cd repo &&
	grit rev-parse --verify --short HEAD >actual &&
	len=$(cat actual | tr -d "\n" | wc -c) &&
	test "$len" -ge 4 &&
	test "$len" -le 40
'

test_expect_success 'verify --short=7 gives exactly 7 chars' '
	cd repo &&
	grit rev-parse --verify --short=7 HEAD >actual &&
	len=$(cat actual | tr -d "\n" | wc -c) &&
	test "$len" = 7
'

test_expect_success 'verify empty string fails' '
	cd repo &&
	test_must_fail grit rev-parse --verify "" 2>err
'

test_expect_success 'HEAD^1 same as HEAD~1 via verify' '
	cd repo &&
	commit1=$(cat commit1.out) &&
	grit rev-parse --verify HEAD^1 >actual1 &&
	grit rev-parse --verify HEAD~1 >actual2 &&
	echo "$commit1" >expect &&
	test_cmp expect actual1 &&
	test_cmp expect actual2
'

test_expect_success 'verify rejects HEAD^2 on non-merge' '
	cd repo &&
	test_must_fail grit rev-parse --verify HEAD^2
'

test_expect_success 'setup third commit for deeper traversal' '
	cd repo &&
	echo three >>hello &&
	grit hash-object -w hello >/dev/null &&
	grit update-index --add hello &&
	tree3=$(grit write-tree) &&
	commit2=$(cat commit2.out) &&
	commit3=$(printf "three\n" | grit commit-tree "$tree3" -p "$commit2") &&
	grit update-ref refs/heads/main "$commit3" &&
	echo "$commit3" >commit3.out
'

test_expect_success 'HEAD~2 resolves to grandparent' '
	cd repo &&
	commit1=$(cat commit1.out) &&
	grit rev-parse --verify HEAD~2 >actual &&
	echo "$commit1" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify HEAD~1 resolves to parent of new head' '
	cd repo &&
	commit2=$(cat commit2.out) &&
	grit rev-parse --verify HEAD~1 >actual &&
	echo "$commit2" >expect &&
	test_cmp expect actual
'

test_expect_success 'verify HEAD^{} peels HEAD (no-op for commit)' '
	cd repo &&
	commit3=$(cat commit3.out) &&
	grit rev-parse --verify "HEAD^{}" >actual &&
	echo "$commit3" >expect &&
	test_cmp expect actual
'

test_done
