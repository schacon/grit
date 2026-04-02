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

# ---------------------------------------------------------------------------
# Giving unrecognized options should fail
# ---------------------------------------------------------------------------
test_expect_success 'giving unrecognized options should fail' '
	cd repo &&
	test_must_fail git reset --other &&
	test_must_fail git reset --mixed --other &&
	test_must_fail git reset --soft --other &&
	test_must_fail git reset --hard --other
'

# ---------------------------------------------------------------------------
# No negated form for various types of reset
# ---------------------------------------------------------------------------
test_expect_success "no git reset --no-soft" '
	cd repo &&
	test_must_fail git reset --no-soft 2>err &&
	test -s err
'

test_expect_success "no git reset --no-mixed" '
	cd repo &&
	test_must_fail git reset --no-mixed 2>err &&
	test -s err
'

test_expect_success "no git reset --no-hard" '
	cd repo &&
	test_must_fail git reset --no-hard 2>err &&
	test -s err
'

# ---------------------------------------------------------------------------
# Disambiguation tests
# ---------------------------------------------------------------------------
test_expect_success 'disambiguation (1) - reset removes staged new file from cache' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&
	>secondfile &&
	git add secondfile &&
	git reset secondfile &&
	test -z "$(git diff --cached --name-only)" &&
	test -f secondfile
'

test_expect_success 'disambiguation (3) - reset HEAD <path> when file deleted' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&
	>secondfile &&
	git add secondfile &&
	rm -f secondfile &&
	git reset HEAD secondfile &&
	test -z "$(git diff --cached --name-only)" &&
	test ! -f secondfile
'

test_expect_success 'disambiguation (4) - reset -- <path> when file deleted' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&
	>secondfile &&
	git add secondfile &&
	rm -f secondfile &&
	git reset -- secondfile &&
	test -z "$(git diff --cached --name-only)" &&
	test ! -f secondfile
'

# ---------------------------------------------------------------------------
# Resetting an unmodified path is a no-op
# ---------------------------------------------------------------------------
test_expect_success 'resetting an unmodified path is a no-op' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&
	git reset -- first &&
	git diff-files --exit-code &&
	git diff-index --cached --exit-code HEAD
'

# ---------------------------------------------------------------------------
# Test resetting the index at given paths
# ---------------------------------------------------------------------------
test_expect_success 'test resetting the index at given paths' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&
	mkdir -p sub &&
	>sub/file1 &&
	>sub/file2 &&
	git update-index --add sub/file1 sub/file2 &&
	T=$(git write-tree) &&
	git reset HEAD sub/file2 &&
	U=$(git write-tree) &&
	test_must_fail git diff-index --cached --exit-code "$T" &&
	test "$T" != "$U"
'

# ---------------------------------------------------------------------------
# Resetting specific path that is unmerged
# ---------------------------------------------------------------------------
test_expect_success 'resetting specific path that is unmerged' '
	cd repo &&
	commit4=$(cat ../commit4) &&
	git reset --hard "$commit4" &&
	F1=$(git rev-parse HEAD:first) &&
	F2=$(git rev-parse HEAD:second) &&
	git rm --cached second &&
	{
		echo "100644 $F1 1	second" &&
		echo "100644 $F2 2	second" &&
		echo "100644 $F1 3	second"
	} | git update-index --index-info &&
	git ls-files -u &&
	git reset HEAD second &&
	git diff-index --exit-code --cached HEAD
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

# ---------------------------------------------------------------------------
# --no-merge and --no-keep also fail (additional negated modes)
# ---------------------------------------------------------------------------
test_expect_success "no git reset --no-merge" '
	cd repo &&
	test_must_fail git reset --no-merge 2>err &&
	test -s err
'

test_expect_success "no git reset --no-keep" '
	cd repo &&
	test_must_fail git reset --no-keep 2>err &&
	test -s err
'

# ---------------------------------------------------------------------------
# Extended setup: repo3 with rm/mv/rename commits for diff-oriented tests
# ---------------------------------------------------------------------------
test_expect_success 'setup repo3 for diff-oriented reset tests' '
	git init repo3 &&
	cd repo3 &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	test_tick &&
	echo "1st file" >first &&
	git add first &&
	git commit -m "create 1st file" &&

	echo "2nd file" >second &&
	git add second &&
	git commit -m "create 2nd file" &&

	echo "2nd line 1st file" >>first &&
	git commit -a -m "modify 1st file" &&
	git rev-parse HEAD >../r3_head5p2 &&

	git rm first &&
	git mv second secondfile &&
	git commit -a -m "remove 1st and rename 2nd" &&
	git rev-parse HEAD >../r3_head5p1 &&

	echo "1st line 2nd file" >secondfile &&
	echo "2nd line 2nd file" >>secondfile &&
	git commit -a -m "modify 2nd file (geandert)" &&
	git rev-parse HEAD >../r3_head5
'

# ---------------------------------------------------------------------------
# --soft reset shows changes in diff --cached
# ---------------------------------------------------------------------------
test_expect_success '--soft reset only should show changes in diff --cached' '
	cd repo3 &&
	head5=$(cat ../r3_head5) &&
	head5p1=$(cat ../r3_head5p1) &&
	git reset --hard "$head5" &&

	git reset --soft HEAD^ &&
	test "$(git rev-parse HEAD)" = "$head5p1" &&

	# Working tree diff should be empty (soft does not touch worktree)
	git diff >wt_diff &&
	test_must_be_empty wt_diff &&

	# Cached diff should show the change from head5p1 to head5
	git diff --cached >cached_diff &&
	test -s cached_diff &&
	grep "1st line 2nd file" cached_diff &&
	grep "2nd line 2nd file" cached_diff &&

	# ORIG_HEAD should be head5
	test "$(git rev-parse ORIG_HEAD)" = "$head5" &&

	# Restore head5
	git reset --hard "$head5"
'

# ---------------------------------------------------------------------------
# --hard reset should change files and undo commits permanently
# ---------------------------------------------------------------------------
test_expect_success '--hard reset should change the files and undo commits permanently' '
	cd repo3 &&
	head5=$(cat ../r3_head5) &&
	head5p2=$(cat ../r3_head5p2) &&

	# Add an extra commit on top of head5
	echo "3rd line 2nd file" >>secondfile &&
	git commit -a -m "add 3rd line" &&
	head4=$(git rev-parse HEAD) &&

	# Reset back to head5p2 (3 commits back: head5p2 -> head5p1 -> head5 -> head4)
	git reset --hard "$head5p2" &&
	test "$(git rev-parse HEAD)" = "$head5p2" &&
	test "$(git rev-parse ORIG_HEAD)" = "$head4" &&

	# Working tree should match head5p2 (first and second exist)
	test -f first &&
	test -f second &&
	test "$(cat first)" = "1st file
2nd line 1st file" &&
	test "$(cat second)" = "2nd file" &&
	test ! -f secondfile
'

# ---------------------------------------------------------------------------
# Redoing changes and adding without committing should succeed
# ---------------------------------------------------------------------------
test_expect_success 'redoing changes adding them without committing should succeed' '
	cd repo3 &&
	head5p2=$(cat ../r3_head5p2) &&

	# We are now at head5p2 after previous hard reset
	test "$(git rev-parse HEAD)" = "$head5p2" &&

	git rm first &&
	git mv second secondfile &&
	echo "1st line 2nd file" >secondfile &&
	echo "2nd line 2nd file" >>secondfile &&
	git add secondfile &&

	# Cached diff should show deletions of first/second and addition of secondfile
	git diff --cached >cached_out &&
	grep "deleted file" cached_out &&
	grep "new file" cached_out &&

	# Working tree diff should be empty
	git diff >wt_out &&
	test_must_be_empty wt_out &&

	# HEAD should still be head5p2
	test "$(git rev-parse HEAD)" = "$head5p2"
'

# ---------------------------------------------------------------------------
# --mixed reset to HEAD should unadd the files
# ---------------------------------------------------------------------------
test_expect_success '--mixed reset to HEAD should unadd the files' '
	cd repo3 &&
	head5p2=$(cat ../r3_head5p2) &&

	# Index still has staged changes from previous test
	git reset &&

	# After mixed reset, cached diff should be empty
	git diff --cached >cached_out &&
	test_must_be_empty cached_out &&

	# Working tree should still show diffs (first deleted, second deleted, secondfile untracked)
	git diff >wt_out &&
	test -s wt_out &&

	# HEAD unchanged
	test "$(git rev-parse HEAD)" = "$head5p2"
'

# ---------------------------------------------------------------------------
# Redoing the last two commits should succeed
# ---------------------------------------------------------------------------
test_expect_success 'redoing the last two commits should succeed' '
	cd repo3 &&
	head5p2=$(cat ../r3_head5p2) &&

	git add secondfile &&
	git reset --hard "$head5p2" &&

	git rm first &&
	git mv second secondfile &&
	git commit -a -m "remove 1st and rename 2nd" &&

	echo "1st line 2nd file" >secondfile &&
	echo "2nd line 2nd file" >>secondfile &&
	echo "modify 2nd file (geandert)" | git commit -a -F - &&

	# The tree at HEAD should only have secondfile
	test ! -f first &&
	test ! -f second &&
	test -f secondfile &&
	test "$(cat secondfile)" = "1st line 2nd file
2nd line 2nd file"
'

# ---------------------------------------------------------------------------
# test --mixed <paths> (from upstream)
# ---------------------------------------------------------------------------
test_expect_success 'test --mixed <paths>' '
	cd repo3 &&
	git reset --hard &&

	echo 1 >file1 &&
	echo 2 >file2 &&
	git add file1 file2 &&
	test_tick &&
	git commit -m files &&

	git rm file2 &&
	echo 3 >file3 &&
	echo 4 >file4 &&
	echo 5 >file1 &&
	git add file1 file3 file4 &&

	git reset HEAD -- file1 file2 file3 &&

	# file1 should be modified in working tree but not staged
	# file2 should be deleted in working tree but not staged
	# file3 should be untracked
	# file4 should remain staged (new file)
	git diff --cached --name-only >cached_names &&
	grep "file4" cached_names &&
	test_must_fail grep "file1" cached_names &&
	test_must_fail grep "file2" cached_names &&
	test_must_fail grep "file3" cached_names
'

# ---------------------------------------------------------------------------
# Additional reset tests
# ---------------------------------------------------------------------------

test_expect_success 'reset with no args defaults to mixed HEAD' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	echo extra >>first &&
	git add first &&
	git diff --cached --name-only >before &&
	grep first before &&
	git reset &&
	git diff --cached --name-only >after &&
	test_must_be_empty after &&
	# Worktree should still have modification
	grep extra first
'

test_expect_success 'reset HEAD is same as reset with no args' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	echo extra >>first &&
	git add first &&
	git reset HEAD &&
	git diff --cached --name-only >after &&
	test_must_be_empty after &&
	grep extra first
'

test_expect_success 'reset --quiet --hard suppresses output' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	git reset --hard --quiet $(cat ../commit1) >stdout 2>stderr &&
	test_must_be_empty stdout &&
	test_must_be_empty stderr &&
	test "$(git rev-parse HEAD)" = "$(cat ../commit1)" &&
	git reset --hard $(cat ../commit4)
'

test_expect_success 'reset --soft does not change index or worktree' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	echo modified >first &&
	git add first &&
	MOD_BLOB=$(git ls-files -s first | awk "{print \$2}") &&
	git reset --soft $(cat ../commit2) &&
	test "$(git rev-parse HEAD)" = "$(cat ../commit2)" &&
	# Index should still have modified blob
	ACTUAL_BLOB=$(git ls-files -s first | awk "{print \$2}") &&
	test "$ACTUAL_BLOB" = "$MOD_BLOB" &&
	# Worktree should still have modified content
	test "$(cat first)" = "modified" &&
	git reset --hard $(cat ../commit4)
'

test_expect_success 'reset --mixed resets index but not worktree' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	echo modified >first &&
	git add first &&
	git reset --mixed $(cat ../commit2) &&
	test "$(git rev-parse HEAD)" = "$(cat ../commit2)" &&
	# Index should match commit2 (no staged changes)
	git diff-index --cached --exit-code HEAD &&
	# Worktree should still have modified content
	test "$(cat first)" = "modified" &&
	git reset --hard $(cat ../commit4)
'

test_expect_success 'reset --hard to ORIG_HEAD' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	git reset --hard $(cat ../commit1) &&
	test "$(git rev-parse ORIG_HEAD)" = "$(cat ../commit4)" &&
	git reset --hard ORIG_HEAD &&
	test "$(git rev-parse HEAD)" = "$(cat ../commit4)"
'

test_expect_success 'reset --hard removes added file' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	echo brand-new >brand-new &&
	git add brand-new &&
	git reset --hard &&
	test_path_is_missing brand-new
'

test_expect_success 'reset -- multiple paths' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	echo x >first &&
	echo y >second &&
	git add first second &&
	git reset HEAD -- first second &&
	git diff --cached --name-only >staged &&
	test_must_be_empty staged &&
	# Worktree should still have changes
	test "$(cat first)" = "x" &&
	test "$(cat second)" = "y"
'

test_expect_success 'reset <commit> -- <path> does not move HEAD' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	git reset $(cat ../commit1) -- first &&
	# HEAD should stay at commit4
	test "$(git rev-parse HEAD)" = "$(cat ../commit4)" &&
	# Index for first should now match commit1 state
	C1_BLOB=$(git rev-parse $(cat ../commit1):first) &&
	ACTUAL_BLOB=$(git ls-files -s first | awk "{print \$2}") &&
	test "$ACTUAL_BLOB" = "$C1_BLOB"
'

test_expect_success 'reset to same commit is a no-op for HEAD' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	git reset --soft HEAD &&
	test "$(git rev-parse HEAD)" = "$(cat ../commit4)" &&
	git diff-index --cached --exit-code HEAD
'

test_expect_success 'reset --hard with dirty worktree discards changes' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	echo dirty >first &&
	echo dirty >second &&
	git reset --hard &&
	test_must_fail grep dirty first &&
	test_must_fail grep dirty second
'

test_expect_success 'reset --soft then commit creates new commit at reset point' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	git reset --soft $(cat ../commit2) &&
	git commit -m "new-after-soft-reset" &&
	# Parent should be commit2
	PARENT=$(git rev-parse HEAD~1) &&
	test "$PARENT" = "$(cat ../commit2)" &&
	git reset --hard $(cat ../commit4)
'

test_expect_success 'reset nonexistent ref fails' '
	cd repo &&
	test_must_fail git reset --hard nonexistent-ref
'

test_expect_success 'reset --hard leaves worktree clean' '
	cd repo &&
	git reset --hard $(cat ../commit4) &&
	echo dirty1 >first &&
	echo dirty2 >second &&
	echo new >newfile &&
	git add first second newfile &&
	git reset --hard &&
	git diff-index --exit-code HEAD &&
	test_path_is_missing newfile
'

test_done
