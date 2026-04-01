#!/bin/sh
# Ported from git/t/t1400-update-ref.sh (harness-compatible subset).

test_description='grit update-ref basics'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

A=1111111111111111111111111111111111111111
B=2222222222222222222222222222222222222222
Z=0000000000000000000000000000000000000000

head_ref_path() {
	sed -n 's/^ref: //p' .git/HEAD
}

test_expect_success 'setup repository' '
	grit init repo &&
	cd repo
'

test_expect_success 'create refs/heads/main' '
	cd repo &&
	grit update-ref refs/heads/main "$A" &&
	echo "$A" >expect &&
	cat .git/refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'update refs/heads/main with old-value verification' '
	cd repo &&
	grit update-ref refs/heads/main "$B" "$A" &&
	echo "$B" >expect &&
	cat .git/refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'stale old-value blocks delete' '
	cd repo &&
	test_must_fail grit update-ref -d refs/heads/main "$A" &&
	echo "$B" >expect &&
	cat .git/refs/heads/main >actual &&
	test_cmp expect actual
'

test_expect_success 'delete ref with correct old-value' '
	cd repo &&
	grit update-ref -d refs/heads/main "$B" &&
	test_path_is_missing .git/refs/heads/main
'

test_expect_success 'updating HEAD dereferences to current branch' '
	cd repo &&
	grit update-ref HEAD "$A" &&
	head_ref=$(head_ref_path) &&
	echo "$A" >expect &&
	cat ".git/$head_ref" >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin accepts empty input' '
	cd repo &&
	: >stdin &&
	grit update-ref --stdin <stdin &&
	head_ref=$(head_ref_path) &&
	echo "$A" >expect &&
	cat ".git/$head_ref" >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin create works' '
	cd repo &&
	echo "create refs/heads/topic $B" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$B" >expect &&
	cat .git/refs/heads/topic >actual &&
	test_cmp expect actual
'

test_expect_success '--stdin update with zero old-value creates ref' '
	cd repo &&
	echo "update refs/heads/newref $A $Z" >stdin &&
	grit update-ref --stdin <stdin &&
	echo "$A" >expect &&
	cat .git/refs/heads/newref >actual &&
	test_cmp expect actual
'

test_expect_success 'transaction start/create/commit reports status' '
	cd repo &&
	cat >stdin <<-\EOF &&
	start
	create refs/heads/txref 3333333333333333333333333333333333333333
	commit
	EOF
	grit update-ref --stdin <stdin >actual &&
	cat >expect <<-\EOF &&
	start: ok
	commit: ok
	EOF
	test_cmp expect actual &&
	echo 3333333333333333333333333333333333333333 >expect &&
	cat .git/refs/heads/txref >actual &&
	test_cmp expect actual
'

test_done
