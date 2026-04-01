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

test_done
