#!/bin/sh
# Ported from git/t/t1403-show-ref.sh (harness-compatible subset).

test_description='show-ref'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	grit init repo &&
	cd repo &&
	tree=$(git write-tree) &&
	commit=$(echo base | git commit-tree "$tree") &&
	grit update-ref refs/heads/master "$commit" &&
	grit update-ref refs/heads/main "$commit" &&
	grit update-ref refs/heads/side "$commit" &&
	grit update-ref refs/heads/topic "$commit" &&
	grit update-ref refs/tags/v1 "$commit"
'

# --- existing tests ---

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

# --- new tests ---

test_expect_success 'show-ref -q pattern match suppresses output' '
	cd repo &&
	git show-ref -q master >actual &&
	test_must_be_empty actual &&
	git show-ref -q refs/heads/master >actual &&
	test_must_be_empty actual
'

test_expect_success 'show-ref -q missing ref fails with empty output' '
	cd repo &&
	test_must_fail git show-ref -q does-not-exist >actual &&
	test_must_be_empty actual
'

test_expect_success 'show-ref --verify -q existing ref gives empty output' '
	cd repo &&
	git show-ref --verify -q refs/heads/side >actual &&
	test_must_be_empty actual
'

test_expect_success 'show-ref --verify fails for non-full-path with empty output' '
	cd repo &&
	test_must_fail git show-ref --verify master >actual &&
	test_must_be_empty actual
'

test_expect_success 'show-ref patterns match tag refs' '
	cd repo &&
	echo "$(git rev-parse refs/tags/v1) refs/tags/v1" >expect &&
	git show-ref v1 >actual &&
	test_cmp expect actual
'

test_expect_success 'show-ref multiple patterns' '
	cd repo &&
	{
		echo "$(git rev-parse refs/heads/master) refs/heads/master" &&
		echo "$(git rev-parse refs/tags/v1) refs/tags/v1"
	} >expect &&
	git show-ref master v1 >actual &&
	test_cmp expect actual
'

test_expect_success 'show-ref --tags' '
	cd repo &&
	echo "$(git rev-parse refs/tags/v1) refs/tags/v1" >expect &&
	git show-ref --tags >actual &&
	test_cmp expect actual
'

test_expect_success 'show-ref --head without pattern shows HEAD first' '
	cd repo &&
	git show-ref --head >actual &&
	head -1 actual >first &&
	grep "^.* HEAD$" first
'

test_expect_success 'show-ref --hash with exact ref prints only oid' '
	cd repo &&
	oid=$(git rev-parse refs/heads/master) &&
	echo "$oid" >expect &&
	git show-ref --hash refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'show-ref --verify --hash prints only oid' '
	cd repo &&
	oid=$(git rev-parse refs/heads/master) &&
	echo "$oid" >expect &&
	git show-ref --verify --hash refs/heads/master >actual &&
	test_cmp expect actual
'

test_expect_success 'show-ref -d with non-tag commit ref' '
	cd repo &&
	echo "$(git rev-parse refs/heads/master) refs/heads/master" >expect &&
	git show-ref -d master >actual &&
	test_cmp expect actual
'

test_expect_success 'show-ref --verify pseudorefs (CHERRY_PICK_HEAD)' '
	cd repo &&
	oid=$(git rev-parse refs/heads/master) &&
	git update-ref CHERRY_PICK_HEAD "$oid" &&
	git show-ref -s --verify CHERRY_PICK_HEAD >actual &&
	echo "$oid" >expect &&
	test_cmp expect actual
'

test_expect_success 'show-ref --verify HEAD with -q' '
	cd repo &&
	git show-ref --verify -q HEAD >actual &&
	test_must_be_empty actual
'

test_expect_success 'show-ref --verify with dangling ref' '
	cd repo &&
	grit init dangling &&
	cd dangling &&
	tree=$(git write-tree) &&
	commit=$(echo dangling | git commit-tree "$tree") &&
	grit update-ref refs/heads/master "$commit" &&
	grit update-ref refs/tags/dangling "$commit" &&
	sha=$(git rev-parse refs/tags/dangling) &&
	file=$(echo "$sha" | sed "s#..#.git/objects/&/#") &&
	test_path_is_file "$file" &&
	rm -f "$file" &&
	test_must_fail git show-ref --verify refs/tags/dangling
'

test_expect_success 'show-ref sub-modes are mutually exclusive' '
	cd repo &&
	test_must_fail git show-ref --verify --exists 2>err &&
	grep "cannot be used together" err
'

test_done
