#!/bin/sh
# Ported from git/t/t1403-show-ref.sh (harness-compatible subset).

test_description='show-ref'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	gust init repo &&
	cd repo &&
	tree=$(git write-tree) &&
	commit=$(echo base | git commit-tree "$tree") &&
	gust update-ref refs/heads/master "$commit" &&
	gust update-ref refs/heads/main "$commit" &&
	gust update-ref refs/heads/side "$commit" &&
	gust update-ref refs/heads/topic "$commit"
'

test_expect_success 'show-ref pattern matching and missing ref exit' '
	cd repo &&
	echo "$(git rev-parse refs/heads/side) refs/heads/side" >expect &&
	git show-ref side >actual &&
	test_cmp expect actual &&
	test_must_fail git show-ref does-not-exist >actual 2>err &&
	test_path_is_file err &&
	test_must_fail test -s actual
'

test_expect_success 'show-ref --verify exact path' '
	cd repo &&
	echo "$(git rev-parse refs/heads/side) refs/heads/side" >expect &&
	git show-ref --verify refs/heads/side >actual &&
	test_cmp expect actual &&
	test_must_fail git show-ref --verify side
'

test_expect_success 'show-ref --verify -q suppresses output' '
	cd repo &&
	git show-ref --verify -q refs/heads/side >actual &&
	test_must_fail test -s actual &&
	test_must_fail git show-ref --verify -q does-not-exist
'

test_expect_success 'show-ref --branches and --head' '
	cd repo &&
	git show-ref --branches >actual &&
	test_path_is_file actual &&
	git show-ref --branches --head >actual &&
	test_path_is_file actual
'

test_expect_success 'show-ref --hash only prints oid' '
	cd repo &&
	git show-ref --hash refs/heads/side >actual &&
	test_path_is_file actual &&
	test_must_fail grep "refs/" actual
'

test_expect_success 'show-ref --verify HEAD' '
	cd repo &&
	echo "$(git rev-parse HEAD) HEAD" >expect &&
	git show-ref --verify HEAD >actual &&
	test_cmp expect actual
'

test_done
