#!/bin/sh
# Ported from git/t/t1401-symbolic-ref.sh (harness-compatible subset).

test_description='basic symbolic-ref tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository' '
	grit init repo &&
	cd repo &&
	git symbolic-ref HEAD refs/heads/main &&
	tree=$(git write-tree) &&
	commit=$(echo initial | git commit-tree "$tree") &&
	grit update-ref refs/heads/main "$commit"
'

test_expect_success 'symbolic-ref read/write roundtrip' '
	cd repo &&
	git symbolic-ref HEAD refs/heads/read-write-roundtrip &&
	echo refs/heads/read-write-roundtrip >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'symbolic-ref refuses non-ref for HEAD' '
	cd repo &&
	test_must_fail git symbolic-ref HEAD foo
'

test_expect_success 'HEAD cannot be removed' '
	cd repo &&
	test_must_fail git symbolic-ref -d HEAD
'

test_expect_success 'symbolic-ref can be deleted' '
	cd repo &&
	git symbolic-ref NOTHEAD refs/heads/read-write-roundtrip &&
	git symbolic-ref -d NOTHEAD &&
	test_must_fail git symbolic-ref NOTHEAD
'

test_expect_success 'symbolic-ref fails to delete missing ref' '
	cd repo &&
	test_must_fail git symbolic-ref -d FOO
'

test_expect_success 'symbolic-ref refuses invalid target for non-HEAD' '
	cd repo &&
	test_must_fail git symbolic-ref refs/heads/invalid foo..bar
'

test_expect_success 'symbolic-ref allows top-level target for non-HEAD' '
	cd repo &&
	oid=$(git rev-parse refs/heads/main) &&
	git symbolic-ref refs/heads/top-level ORIG_HEAD &&
	grit update-ref ORIG_HEAD "$oid" &&
	git symbolic-ref refs/heads/top-level >actual &&
	echo ORIG_HEAD >expect &&
	test_cmp expect actual
'

test_expect_success 'symbolic-ref pointing at another with --no-recurse' '
	cd repo &&
	git symbolic-ref refs/heads/maint refs/heads/main &&
	git symbolic-ref HEAD refs/heads/maint &&
	git symbolic-ref HEAD >actual &&
	echo refs/heads/main >expect &&
	test_cmp expect actual &&
	git symbolic-ref --no-recurse HEAD >actual &&
	echo refs/heads/maint >expect &&
	test_cmp expect actual
'

test_expect_success 'symbolic-ref --short handles names' '
	cd repo &&
	git symbolic-ref TEST_SYMREF refs/heads/main &&
	git symbolic-ref --short TEST_SYMREF >actual &&
	echo main >expect &&
	test_cmp expect actual
'

test_done
