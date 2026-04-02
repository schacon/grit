#!/bin/sh
#
# Tests for 'checkout $tree -- $paths' — checking out specific paths from commits.
# Adapted from git/t/t2022-checkout-paths.sh

test_description='checkout $tree -- $paths'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup: two branches with different directory contents
# ---------------------------------------------------------------------------
test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	mkdir dir &&
	echo master-main >dir/main &&
	echo common >dir/common &&
	git add dir/main dir/common &&
	git commit -m "master has dir/main" &&
	git rev-parse HEAD >../master_oid &&

	git checkout -b next &&
	git rm dir/main &&
	echo next >dir/next1 &&
	git add dir &&
	git commit -m "next has dir/next but not dir/main" &&
	git rev-parse HEAD >../next_oid
'

# ---------------------------------------------------------------------------
# Checking out paths from another branch brings in files
# ---------------------------------------------------------------------------
test_expect_success 'checkout branch -- dir brings in files from that branch' '
	cd repo &&
	git checkout next &&
	git reset --hard &&

	git checkout master -- dir &&

	test -f dir/main &&
	echo master-main >expect &&
	test_cmp expect dir/main &&
	echo common >expect_common &&
	test_cmp expect_common dir/common
'

# ---------------------------------------------------------------------------
# checkout HEAD -- file restores file content
# ---------------------------------------------------------------------------
test_expect_success 'checkout HEAD -- file restores content' '
	cd repo &&
	git checkout master &&
	git reset --hard &&

	echo modified >dir/main &&
	git checkout HEAD -- dir/main &&
	echo master-main >expect &&
	test_cmp expect dir/main
'

# ---------------------------------------------------------------------------
# checkout <commit> -- path from an older commit
# ---------------------------------------------------------------------------
test_expect_success 'checkout <commit> -- path restores older version' '
	cd repo &&
	git checkout master &&
	git reset --hard &&
	echo "second version" >dir/main &&
	git commit -a -m "update dir/main" &&

	master_oid=$(cat ../master_oid) &&
	git checkout "$master_oid" -- dir/main &&
	echo master-main >expect &&
	test_cmp expect dir/main
'

# ---------------------------------------------------------------------------
# checkout from tree stages the file
# ---------------------------------------------------------------------------
test_expect_success 'checkout <tree> -- path stages the file' '
	cd repo &&
	git checkout master &&
	git reset --hard &&

	git checkout next -- dir/next1 &&
	git diff --cached --name-only >staged &&
	grep "dir/next1" staged
'

# ---------------------------------------------------------------------------
# checkout -- path does not move HEAD
# ---------------------------------------------------------------------------
test_expect_success 'checkout <tree> -- path does not move HEAD' '
	cd repo &&
	git checkout master &&
	git reset --hard &&
	before=$(git rev-parse HEAD) &&
	git checkout next -- dir/next1 &&
	after=$(git rev-parse HEAD) &&
	test "$before" = "$after"
'

# ---------------------------------------------------------------------------
# checkout -- path preserves unrelated working tree changes
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- path preserves unrelated working tree changes' '
	cd repo &&
	git checkout master &&
	git reset --hard &&

	echo local-change >dir/common &&
	git checkout next -- dir/next1 &&
	echo local-change >expect &&
	test_cmp expect dir/common
'

# ---------------------------------------------------------------------------
# checkout HEAD -- path on unmodified file is a no-op
# ---------------------------------------------------------------------------
test_expect_success 'checkout HEAD -- on clean file is a no-op' '
	cd repo &&
	git checkout master &&
	git reset --hard &&
	git checkout HEAD -- dir/main &&
	git diff --exit-code &&
	git diff --cached --exit-code
'

# ---------------------------------------------------------------------------
# checkout -- multiple paths
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- multiple paths at once' '
	cd repo &&
	git checkout master &&
	git reset --hard &&

	# Save current HEAD content for comparison
	cat dir/main >saved_main &&
	cat dir/common >saved_common &&

	echo mod1 >dir/main &&
	echo mod2 >dir/common &&
	git checkout HEAD -- dir/main dir/common &&
	test_cmp saved_main dir/main &&
	test_cmp saved_common dir/common
'

# ---------------------------------------------------------------------------
# checkout branch -- path adds file that does not exist on current branch
# ---------------------------------------------------------------------------
test_expect_success 'checkout from other branch adds missing file' '
	cd repo &&
	git checkout master &&
	git reset --hard &&

	test_path_is_missing dir/next1 &&
	git checkout next -- dir/next1 &&
	test -f dir/next1 &&
	echo next >expect &&
	test_cmp expect dir/next1
'

# ---------------------------------------------------------------------------
# checkout path that does not exist in tree fails
# ---------------------------------------------------------------------------
test_expect_success 'checkout nonexistent path from tree fails' '
	cd repo &&
	git checkout master &&
	git reset --hard &&
	test_must_fail git checkout HEAD -- nonexistent 2>err &&
	test -s err
'

# ---------------------------------------------------------------------------
# checkout -- path with directory argument
# ---------------------------------------------------------------------------
test_expect_success 'checkout HEAD -- dir restores entire directory' '
	cd repo &&
	git checkout master &&
	git reset --hard &&

	# Save current HEAD content for comparison
	cat dir/main >saved_main &&
	cat dir/common >saved_common &&

	echo changed >dir/main &&
	echo changed >dir/common &&
	git checkout HEAD -- dir &&
	test_cmp saved_main dir/main &&
	test_cmp saved_common dir/common
'

test_done
