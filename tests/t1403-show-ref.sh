#!/bin/sh
# Ported from git/t/t1403-show-ref.sh (harness-compatible subset).

test_description='show-ref'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- setup (mirrors upstream but uses master as default branch) ---

test_expect_success 'setup' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test User" &&
	tree=$(git write-tree) &&
	commit_A=$(echo A | git commit-tree "$tree") &&
	grit update-ref refs/heads/master "$commit_A" &&
	grit tag -a -m "tag A" A "$commit_A" &&
	git symbolic-ref HEAD refs/heads/side &&
	commit_B=$(echo B | git commit-tree "$tree" -p "$commit_A") &&
	grit update-ref refs/heads/side "$commit_B" &&
	grit tag -a -m "tag B" B "$commit_B" &&
	git symbolic-ref HEAD refs/heads/master &&
	commit_C=$(echo C | git commit-tree "$tree" -p "$commit_A") &&
	grit update-ref refs/heads/master "$commit_C" &&
	grit tag C "$commit_C" &&
	git branch B "$commit_A"
'

# --- show-ref (pattern matching) ---

test_expect_success 'show-ref' '
	cd repo &&
	echo "$(git rev-parse refs/tags/A) refs/tags/A" >expect &&

	git show-ref A >actual &&
	test_cmp expect actual &&

	git show-ref tags/A >actual &&
	test_cmp expect actual &&

	git show-ref refs/tags/A >actual &&
	test_cmp expect actual &&

	test_must_fail git show-ref D >actual &&
	test_must_be_empty actual
'

# --- show-ref -q ---

test_expect_success 'show-ref -q' '
	cd repo &&
	git show-ref -q A >actual &&
	test_must_be_empty actual &&

	git show-ref -q tags/A >actual &&
	test_must_be_empty actual &&

	git show-ref -q refs/tags/A >actual &&
	test_must_be_empty actual &&

	test_must_fail git show-ref -q D >actual &&
	test_must_be_empty actual
'

# --- show-ref --verify ---

test_expect_success 'show-ref --verify' '
	cd repo &&
	echo "$(git rev-parse refs/tags/A) refs/tags/A" >expect &&

	git show-ref --verify refs/tags/A >actual &&
	test_cmp expect actual &&

	test_must_fail git show-ref --verify A >actual &&
	test_must_be_empty actual &&

	test_must_fail git show-ref --verify tags/A >actual &&
	test_must_be_empty actual &&

	test_must_fail git show-ref --verify D >actual &&
	test_must_be_empty actual
'

# --- show-ref --verify -q ---

test_expect_success 'show-ref --verify -q' '
	cd repo &&
	git show-ref --verify -q refs/tags/A >actual &&
	test_must_be_empty actual &&

	test_must_fail git show-ref --verify -q A >actual &&
	test_must_be_empty actual &&

	test_must_fail git show-ref --verify -q tags/A >actual &&
	test_must_be_empty actual &&

	test_must_fail git show-ref --verify -q D >actual &&
	test_must_be_empty actual
'

# --- show-ref -d (dereference annotated tags) ---

test_expect_success 'show-ref -d' '
	cd repo &&
	{
		echo "$(git rev-parse refs/tags/A) refs/tags/A" &&
		echo "$(git rev-parse "refs/tags/A^{}") refs/tags/A^{}" &&
		echo "$(git rev-parse refs/tags/C) refs/tags/C"
	} >expect &&
	git show-ref -d A C >actual &&
	test_cmp expect actual &&

	git show-ref -d tags/A tags/C >actual &&
	test_cmp expect actual &&

	git show-ref -d refs/tags/A refs/tags/C >actual &&
	test_cmp expect actual &&

	git show-ref --verify -d refs/tags/A refs/tags/C >actual &&
	test_cmp expect actual &&

	echo "$(git rev-parse refs/heads/master) refs/heads/master" >expect &&
	git show-ref -d master >actual &&
	test_cmp expect actual &&

	git show-ref -d heads/master >actual &&
	test_cmp expect actual &&

	git show-ref -d refs/heads/master >actual &&
	test_cmp expect actual &&

	git show-ref -d --verify refs/heads/master >actual &&
	test_cmp expect actual &&

	test_must_fail git show-ref -d --verify master >actual &&
	test_must_be_empty actual &&

	test_must_fail git show-ref -d --verify heads/master >actual &&
	test_must_be_empty actual &&

	test_must_fail git show-ref --verify -d A C >actual &&
	test_must_be_empty actual &&

	test_must_fail git show-ref --verify -d tags/A tags/C >actual &&
	test_must_be_empty actual
'

# --- show-ref --branches ---

test_expect_success 'show-ref --branches' '
	cd repo &&
	for branch in B master side
	do
		echo "$(git rev-parse refs/heads/$branch) refs/heads/$branch" || return 1
	done >expect &&
	git show-ref --branches >actual &&
	test_cmp expect actual
'

# --- show-ref --tags ---

test_expect_success 'show-ref --tags' '
	cd repo &&
	for tag in A B C
	do
		echo "$(git rev-parse refs/tags/$tag) refs/tags/$tag" || return 1
	done >expect &&
	git show-ref --tags >actual &&
	test_cmp expect actual
'

# --- show-ref --head with pattern ---

test_expect_success 'show-ref --head with pattern' '
	cd repo &&
	{
		echo "$(git rev-parse refs/heads/B) refs/heads/B" &&
		echo "$(git rev-parse refs/tags/B) refs/tags/B"
	} >expect &&
	git show-ref --head B >actual &&
	test_cmp expect actual
'

# --- show-ref --head -d with pattern ---

test_expect_success 'show-ref --head -d with pattern' '
	cd repo &&
	{
		echo "$(git rev-parse refs/heads/B) refs/heads/B" &&
		echo "$(git rev-parse refs/tags/B) refs/tags/B" &&
		echo "$(git rev-parse "refs/tags/B^{}") refs/tags/B^{}"
	} >expect &&
	git show-ref --head -d B >actual &&
	test_cmp expect actual
'

# --- show-ref --verify HEAD ---

test_expect_success 'show-ref --verify HEAD' '
	cd repo &&
	echo "$(git rev-parse HEAD) HEAD" >expect &&
	git show-ref --verify HEAD >actual &&
	test_cmp expect actual &&

	git show-ref --verify -q HEAD >actual &&
	test_must_be_empty actual
'

# --- show-ref --verify pseudorefs ---

test_expect_success 'show-ref --verify pseudorefs' '
	cd repo &&
	oid=$(git rev-parse HEAD) &&
	git update-ref CHERRY_PICK_HEAD "$oid" &&
	git show-ref -s --verify HEAD >actual &&
	git show-ref -s --verify CHERRY_PICK_HEAD >expect &&
	test_cmp actual expect
'

# --- show-ref --verify with dangling ref ---

test_expect_success 'show-ref --verify with dangling ref' '
	cd repo &&
	grit init dangling &&
	cd dangling &&
	git config user.email "test@test.com" &&
	git config user.name "Test User" &&
	tree=$(git write-tree) &&
	commit=$(echo dangling | git commit-tree "$tree") &&
	grit update-ref refs/heads/master "$commit" &&
	grit tag dangling "$commit" &&
	sha=$(git rev-parse refs/tags/dangling) &&
	file=$(echo "$sha" | sed "s#..#.git/objects/&/#") &&
	test_path_is_file "$file" &&
	rm -f "$file" &&
	test_must_fail git show-ref --verify refs/tags/dangling
'

# --- show-ref sub-modes are mutually exclusive ---

test_expect_success 'show-ref sub-modes are mutually exclusive' '
	cd repo &&
	test_must_fail git show-ref --verify --exists 2>err &&
	grep "cannot be used together" err
'

# --- show-ref --hash (various forms) ---

test_expect_success 'show-ref --hash only prints oid' '
	cd repo &&
	git show-ref --hash refs/heads/master >actual &&
	test_path_is_file actual &&
	test_must_fail grep "refs/" actual
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

# --- show-ref multiple patterns ---

test_expect_success 'show-ref multiple patterns' '
	cd repo &&
	{
		echo "$(git rev-parse refs/heads/master) refs/heads/master" &&
		echo "$(git rev-parse refs/tags/A) refs/tags/A"
	} >expect &&
	git show-ref master A >actual &&
	test_cmp expect actual
'

# --- show-ref --head without pattern shows HEAD first ---

test_expect_success 'show-ref --head without pattern shows HEAD first' '
	cd repo &&
	git show-ref --head >actual &&
	head -1 actual >first &&
	grep "^.* HEAD$" first
'

# --- show-ref -d with non-tag (commit) ref does NOT add ^{} ---

test_expect_success 'show-ref -d with non-tag commit ref' '
	cd repo &&
	echo "$(git rev-parse refs/heads/master) refs/heads/master" >expect &&
	git show-ref -d master >actual &&
	test_cmp expect actual
'

# --- show-ref -d peels annotated tags ---

test_expect_success 'show-ref -d peels annotated tags' '
	cd repo &&
	tag_oid=$(git rev-parse refs/tags/A) &&
	peeled_oid=$(git rev-parse "refs/tags/A^{}") &&
	{
		echo "$tag_oid refs/tags/A" &&
		echo "$peeled_oid refs/tags/A^{}"
	} >expect &&
	git show-ref -d refs/tags/A >actual &&
	test_cmp expect actual
'

# --- show-ref -d does not peel lightweight tags ---

test_expect_success 'show-ref -d does not peel lightweight tags' '
	cd repo &&
	echo "$(git rev-parse refs/tags/C) refs/tags/C" >expect &&
	git show-ref -d refs/tags/C >actual &&
	test_cmp expect actual
'

# --- show-ref -d --verify with multiple full refs ---

test_expect_success 'show-ref -d --verify with multiple full refs' '
	cd repo &&
	{
		echo "$(git rev-parse refs/tags/A) refs/tags/A" &&
		echo "$(git rev-parse "refs/tags/A^{}") refs/tags/A^{}" &&
		echo "$(git rev-parse refs/tags/C) refs/tags/C"
	} >expect &&
	git show-ref --verify -d refs/tags/A refs/tags/C >actual &&
	test_cmp expect actual
'

# --- show-ref --branches excludes tags ---

test_expect_success 'show-ref --branches excludes tags' '
	cd repo &&
	git show-ref --branches >actual &&
	test_must_fail grep "refs/tags/" actual
'

# --- show-ref --tags excludes branches ---

test_expect_success 'show-ref --tags excludes branches' '
	cd repo &&
	git show-ref --tags >actual &&
	test_must_fail grep "refs/heads/" actual
'

# --- show-ref --head with --verify ---

test_expect_success 'show-ref --verify HEAD with -q' '
	cd repo &&
	git show-ref --verify -q HEAD >actual &&
	test_must_be_empty actual
'

# --- show-ref with no matching pattern returns non-zero ---

test_expect_success 'show-ref returns non-zero for no match' '
	cd repo &&
	test_must_fail git show-ref does-not-exist-anywhere >actual &&
	test_must_be_empty actual
'

# --- show-ref -q pattern match suppresses output ---

test_expect_success 'show-ref -q pattern match suppresses output' '
	cd repo &&
	git show-ref -q master >actual &&
	test_must_be_empty actual &&
	git show-ref -q refs/heads/master >actual &&
	test_must_be_empty actual
'

# --- show-ref -q missing ref fails with empty output ---

test_expect_success 'show-ref -q missing ref fails with empty output' '
	cd repo &&
	test_must_fail git show-ref -q does-not-exist >actual &&
	test_must_be_empty actual
'

# --- show-ref --verify fails for non-full-path with empty output ---

test_expect_success 'show-ref --verify fails for non-full-path with empty output' '
	cd repo &&
	test_must_fail git show-ref --verify master >actual &&
	test_must_be_empty actual
'

# --- show-ref -d with multiple patterns (annotated + lightweight) ---

test_expect_success 'show-ref -d with annotated and lightweight tags' '
	cd repo &&
	{
		echo "$(git rev-parse refs/tags/A) refs/tags/A" &&
		echo "$(git rev-parse "refs/tags/A^{}") refs/tags/A^{}" &&
		echo "$(git rev-parse refs/tags/C) refs/tags/C"
	} >expect &&
	git show-ref -d A C >actual &&
	test_cmp expect actual
'

# --- show-ref --exists ---

test_expect_success 'show-ref --exists with existing ref' '
	cd repo &&
	git show-ref --exists refs/heads/master
'

test_expect_success 'show-ref --exists with missing ref' '
	cd repo &&
	test_must_fail git show-ref --exists refs/heads/does-not-exist
'

test_done
