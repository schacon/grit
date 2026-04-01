#!/bin/sh
# Tests for 'grit reset'.
# Ported from git/t/t7102-reset.sh and git/t/t7104-reset-hard.sh
# (uses only plumbing commands that grit implements)

test_description='grit reset'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup repo with several commits; save OIDs to files for later tests.
# ---------------------------------------------------------------------------
test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'create commits and save OIDs' '
	cd repo &&
	echo "1st file" >first &&
	git add first &&
	git commit -m "create 1st file" &&
	git rev-parse HEAD >../commit1 &&

	echo "2nd file" >second &&
	git add second &&
	git commit -m "create 2nd file" &&
	git rev-parse HEAD >../commit2 &&

	echo "2nd line 1st file" >>first &&
	git commit -a -m "modify 1st file" &&
	git rev-parse HEAD >../commit3 &&

	echo "extra line" >>second &&
	git commit -a -m "modify 2nd file" &&
	git rev-parse HEAD >../commit4
'

# ---------------------------------------------------------------------------
# Invalid revision should fail; state unchanged
# ---------------------------------------------------------------------------
test_expect_success 'giving a non-existing explicit ref should fail' '
	cd repo &&
	current=$(git rev-parse HEAD) &&
	test_must_fail git reset --soft refs/heads/nosucharef &&
	test_must_fail git reset --mixed refs/heads/nosucharef &&
	test_must_fail git reset --hard refs/heads/nosucharef &&
	test "$(git rev-parse HEAD)" = "$current"
'

# ---------------------------------------------------------------------------
# Mode + paths combinations
# ---------------------------------------------------------------------------
test_expect_success 'reset --soft with paths should fail' '
	cd repo &&
	test_must_fail git reset --soft -- second &&
	test_must_fail git reset --soft HEAD -- second
'

test_expect_success 'reset --hard with paths should fail' '
	cd repo &&
	test_must_fail git reset --hard -- second &&
	test_must_fail git reset --hard HEAD -- second
'

# ---------------------------------------------------------------------------
# No-op resets against HEAD leave state unchanged
# ---------------------------------------------------------------------------
test_expect_success 'resetting to HEAD with no changes succeeds and does nothing' '
	cd repo &&
	current=$(git rev-parse HEAD) &&
	git reset --hard && test "$(git rev-parse HEAD)" = "$current" &&
	git reset --hard HEAD && test "$(git rev-parse HEAD)" = "$current" &&
	git reset --soft && test "$(git rev-parse HEAD)" = "$current" &&
	git reset --soft HEAD && test "$(git rev-parse HEAD)" = "$current" &&
	git reset --mixed && test "$(git rev-parse HEAD)" = "$current" &&
	git reset --mixed HEAD && test "$(git rev-parse HEAD)" = "$current" &&
	git reset && test "$(git rev-parse HEAD)" = "$current" &&
	git reset HEAD && test "$(git rev-parse HEAD)" = "$current"
'

# ---------------------------------------------------------------------------
# --hard reset output message
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard prints HEAD is now at message' '
	cd repo &&
	git reset --hard HEAD >.actual &&
	head_hex=$(git rev-parse --short HEAD) &&
	echo "HEAD is now at $head_hex modify 2nd file" >.expected &&
	test_cmp .expected .actual
'

# ---------------------------------------------------------------------------
# --soft reset: HEAD moves, index keeps staged changes, working tree unchanged
# ---------------------------------------------------------------------------
test_expect_success '--soft reset moves HEAD and writes ORIG_HEAD' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	commit3=$(cat ../commit3) &&

	git reset --soft "$commit3" &&
	test "$(git rev-parse HEAD)" = "$commit3" &&
	test "$(git rev-parse ORIG_HEAD)" = "$commit4"
'

test_expect_success '--soft reset leaves index with staged changes from old HEAD' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	# Index tree should match commit4 (the old HEAD), not commit3 (current HEAD)
	index_tree=$(git write-tree) &&
	old_tree=$(git cat-file commit "$commit4" | grep "^tree " | cut -d" " -f2) &&
	test "$index_tree" = "$old_tree"
'

test_expect_success 'restore commit4 after soft reset' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git commit -m "modify 2nd file" &&
	git rev-parse HEAD >../commit4
'

# ---------------------------------------------------------------------------
# --mixed reset: HEAD moves, index resets, working tree unchanged
# ---------------------------------------------------------------------------
test_expect_success '--mixed reset moves HEAD and resets index' '
	cd repo &&
	commit3=$(cat ../commit3) &&
	commit4=$(cat ../commit4) &&

	git reset --mixed "$commit3" &&
	test "$(git rev-parse HEAD)" = "$commit3" &&
	test "$(git rev-parse ORIG_HEAD)" = "$commit4" &&

	# Index tree should now match commit3 (new HEAD)
	index_tree=$(git write-tree) &&
	head_tree=$(git cat-file commit "$commit3" | grep "^tree " | cut -d" " -f2) &&
	test "$index_tree" = "$head_tree"
'

test_expect_success 'restore commit4 after mixed reset' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4"
'

# ---------------------------------------------------------------------------
# --hard reset: moves HEAD, resets index AND working tree
# ---------------------------------------------------------------------------
test_expect_success '--hard reset changes HEAD, index, and files' '
	cd repo &&
	commit2=$(cat ../commit2) &&
	commit4=$(cat ../commit4) &&

	git reset --hard "$commit2" &&
	test "$(git rev-parse HEAD)" = "$commit2" &&
	test "$(git rev-parse ORIG_HEAD)" = "$commit4" &&

	# Working tree should only have first and second (as of commit2)
	test -f first &&
	test -f second &&
	test "$(cat first)" = "1st file" &&
	test "$(cat second)" = "2nd file"
'

test_expect_success '--hard reset removes files not in target tree' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	# commit4 has first with extra line and second with extra line
	git reset --hard "$commit4" &&
	test "$(cat first)" = "1st file
2nd line 1st file" &&
	test "$(cat second)" = "2nd file
extra line"
'

# ---------------------------------------------------------------------------
# ORIG_HEAD written before hard reset
# ---------------------------------------------------------------------------
test_expect_success 'ORIG_HEAD is written by hard reset' '
	cd repo &&
	commit3=$(cat ../commit3) &&
	commit4=$(cat ../commit4) &&
	before=$(git rev-parse HEAD) &&
	test "$before" = "$commit4" &&
	git reset --hard "$commit3" &&
	test "$(git rev-parse ORIG_HEAD)" = "$commit4" &&
	git reset --hard "$commit4"
'

# ---------------------------------------------------------------------------
# Pathspec reset (unstage)
# ---------------------------------------------------------------------------
test_expect_success 'reset -- <path> unstages a staged file' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&

	# Stage a modification to second
	echo "staged extra" >>second &&
	git add second &&

	# Index for second now differs from HEAD
	index_oid=$(git ls-files --stage second | awk "{ print \$2 }") &&
	head_oid=$(git rev-parse HEAD:second) &&
	test "$index_oid" != "$head_oid" &&

	# Unstage via reset
	git reset HEAD -- second &&

	# Index should now match HEAD for second
	index_oid_after=$(git ls-files --stage second | awk "{ print \$2 }") &&
	test "$index_oid_after" = "$head_oid"
'

test_expect_success 'reset -- <path> does not move HEAD' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	current_head=$(git rev-parse HEAD) &&
	echo "mod" >>first &&
	git add first &&
	git reset HEAD -- first &&
	test "$(git rev-parse HEAD)" = "$current_head"
'

test_expect_success 'reset -- <path> preserves working tree changes' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&
	echo "wt-change" >>second &&
	git add second &&
	git reset HEAD -- second &&
	grep "wt-change" second
'

test_expect_success 'reset <treeish> -- <path> sets index to older commit state' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	commit2=$(cat ../commit2) &&
	git reset --hard "$commit4" &&

	# Reset "second" index entry to state at commit2
	git reset "$commit2" -- second &&
	old_oid=$(git rev-parse "$commit2:second") &&
	idx_oid=$(git ls-files --stage second | awk "{ print \$2 }") &&
	test "$idx_oid" = "$old_oid"
'

# ---------------------------------------------------------------------------
# --soft fails when merge is in progress or index has unmerged entries
# ---------------------------------------------------------------------------
test_expect_success 'reset --soft with MERGE_HEAD present fails' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&
	fake_oid=$(git rev-parse HEAD) &&
	printf "%s\n" "$fake_oid" >.git/MERGE_HEAD &&
	test_must_fail git reset --soft HEAD &&
	rm .git/MERGE_HEAD
'

test_expect_success 'reset --soft with unmerged index entry fails' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&
	blob_oid=$(git rev-parse HEAD:second) &&
	printf "100644 %s 1\tsecond\n" "$blob_oid" |
		git update-index --index-info &&
	test_must_fail git reset --soft HEAD &&
	git update-index --force-remove second &&
	git reset HEAD -- second
'

# ---------------------------------------------------------------------------
# t7104: --hard reset with unmerged entries restores clean state
# ---------------------------------------------------------------------------
test_expect_success 'setup repo2 for unmerged-entry test' '
	git init repo2 &&
	cd repo2 &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	mkdir before &&
	echo 1 >before/1 &&
	echo 2 >before/2 &&
	echo hello >hello &&
	mkdir later &&
	echo 3 >later/3 &&
	git add . &&
	git commit -m initial &&
	git write-tree >../init_tree2
'

test_expect_success 'reset --hard restores files from unmerged index state' '
	cd repo2 &&
	hello_oid=$(git rev-parse HEAD:hello) &&

	# Add hello at stage 2 (simulating a merge conflict)
	printf "100644 %s 2\thello\n" "$hello_oid" |
		git update-index --index-info &&

	# Replace hello with a directory in the working tree
	rm hello &&
	mkdir hello &&
	echo world >hello/world &&

	# --hard should restore everything to HEAD state
	git reset --hard &&

	test -f hello &&
	test "$(cat hello)" = "hello" &&
	test -f before/1 &&
	test -f before/2 &&
	test -f later/3 &&

	# Tree must match original
	new_tree=$(git write-tree) &&
	test "$new_tree" = "$(cat ../init_tree2)"
'

test_done
