#!/bin/sh

test_description='restore basic functionality'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	test_commit first &&
	echo first-and-a-half >>first.t &&
	git add first.t &&
	test_commit second &&
	echo one >one &&
	echo two >two &&
	echo untracked >untracked &&
	echo ignored >ignored &&
	echo /ignored >.gitignore &&
	git add one two .gitignore &&
	git update-ref refs/heads/one HEAD
'

test_expect_success 'restore without pathspec is not ok' '
	cd repo &&
	test_must_fail git restore &&
	test_must_fail git restore --source=first
'

test_expect_success 'restore a file, ignoring branch of same name' '
	cd repo &&
	cat one >expected &&
	echo dirty >>one &&
	git restore one &&
	test_cmp expected one
'

test_expect_success 'restore a file on worktree from another ref' '
	cd repo &&
	# Save current HEAD blob for first.t
	FIRST_BLOB=$(git rev-parse first:first.t) &&
	HEAD_BLOB=$(git rev-parse HEAD:first.t) &&
	git cat-file -p "$FIRST_BLOB" >expected_first &&
	git restore --source=first first.t &&
	test_cmp expected_first first.t &&
	# Index should still have HEAD content (worktree-only restore)
	ACTUAL_INDEX_OID=$(git ls-files -s first.t | awk "{print \$2}") &&
	test "$ACTUAL_INDEX_OID" = "$HEAD_BLOB" &&
	# Cleanup: restore worktree from index
	git cat-file -p "$HEAD_BLOB" >first.t
'

test_expect_success 'restore a file in the index from another ref' '
	cd repo &&
	FIRST_BLOB=$(git rev-parse first:first.t) &&
	HEAD_BLOB=$(git rev-parse HEAD:first.t) &&
	git cat-file -p "$FIRST_BLOB" >expected_first &&
	git restore --source=first --staged first.t &&
	# Index should now have "first" content
	ACTUAL_INDEX_OID=$(git ls-files -s first.t | awk "{print \$2}") &&
	test "$ACTUAL_INDEX_OID" = "$FIRST_BLOB" &&
	# Worktree should still have HEAD content
	git cat-file -p "$HEAD_BLOB" >expected_head &&
	test_cmp expected_head first.t &&
	# Cleanup: restore index back to HEAD
	git restore --staged first.t
'

test_expect_success 'restore a file in both the index and worktree from another ref' '
	cd repo &&
	FIRST_BLOB=$(git rev-parse first:first.t) &&
	git cat-file -p "$FIRST_BLOB" >expected_first &&
	git restore --source=first --staged --worktree first.t &&
	# Verify index
	ACTUAL_INDEX_OID=$(git ls-files -s first.t | awk "{print \$2}") &&
	test "$ACTUAL_INDEX_OID" = "$FIRST_BLOB" &&
	# Verify worktree
	test_cmp expected_first first.t &&
	# Cleanup: restore both back to HEAD
	git restore --staged first.t &&
	HEAD_BLOB=$(git rev-parse HEAD:first.t) &&
	git cat-file -p "$HEAD_BLOB" >first.t
'

test_expect_success 'restore --staged uses HEAD as source' '
	cd repo &&
	HEAD_BLOB=$(git rev-parse HEAD:first.t) &&
	git cat-file -p "$HEAD_BLOB" >expected &&
	echo index-dirty >>first.t &&
	git add first.t &&
	git restore --staged first.t &&
	# Index should match HEAD
	ACTUAL_INDEX_OID=$(git ls-files -s first.t | awk "{print \$2}") &&
	test "$ACTUAL_INDEX_OID" = "$HEAD_BLOB" &&
	# Cleanup: restore worktree
	git cat-file -p "$HEAD_BLOB" >first.t
'

test_expect_success 'restore --worktree --staged uses HEAD as source' '
	cd repo &&
	HEAD_BLOB=$(git rev-parse HEAD:first.t) &&
	git cat-file -p "$HEAD_BLOB" >expected &&
	echo dirty >>first.t &&
	git add first.t &&
	git restore --worktree --staged first.t &&
	# Verify index matches HEAD
	ACTUAL_INDEX_OID=$(git ls-files -s first.t | awk "{print \$2}") &&
	test "$ACTUAL_INDEX_OID" = "$HEAD_BLOB" &&
	# Verify worktree matches HEAD
	test_cmp expected first.t
'

test_expect_success 'restore --staged adds deleted intent-to-add file back to index' '
	cd repo &&
	echo "nonempty" >nonempty &&
	>empty &&
	git add nonempty empty &&
	git commit -m "create files to be deleted" &&
	# Remove from index (simulate git rm --cached)
	git update-index --force-remove nonempty empty &&
	# Add as intent-to-add
	git add -N nonempty empty &&
	git restore --staged nonempty empty &&
	# Verify no staged changes (index should match HEAD)
	git diff-index --cached --exit-code HEAD
'

test_expect_success 'restore --staged removes file not in HEAD from index' '
	git init subrepo &&
	(
		cd subrepo &&
		git config user.name "Test User" &&
		git config user.email "test@test.com" &&
		echo content >f1 &&
		git add f1 &&
		git commit -m "add f1" &&
		# Stage f2 but it is not in HEAD
		echo content >f2 &&
		git add f2 &&
		# Verify f2 is staged
		F2_OID=$(git ls-files -s f2 | awk "{print \$2}") &&
		test -n "$F2_OID" &&
		# Restore --staged should remove f2 from index (HEAD has no f2)
		git restore --staged f2 &&
		# f2 should no longer be in the index
		test -z "$(git ls-files -s f2)"
	)
'

test_expect_success 'restore --staged invalidates cache tree for deletions' '
	cd repo &&
	git reset --hard &&
	>new1 &&
	>new2 &&
	git add new1 new2 &&

	# Commit and then soft-reset so index has a valid cache-tree for both files
	git commit -m both &&
	git reset --soft HEAD^ &&

	git restore --staged new1 &&
	git commit -m "just new2" &&
	git rev-parse HEAD:new2 &&
	test_must_fail git rev-parse HEAD:new1
'

test_expect_success 'restore --ignore-unmerged ignores unmerged entries' '
	git init unmerged &&
	(
		cd unmerged &&
		git config user.name "Test User" &&
		git config user.email "test@test.com" &&
		echo one >unmerged &&
		echo one >common &&
		git add unmerged common &&
		git commit -m common &&

		# Simulate merge conflict via update-index
		O=$(git hash-object -w unmerged) &&
		echo first >unmerged &&
		A=$(git hash-object -w unmerged) &&
		echo second >unmerged &&
		B=$(git hash-object -w unmerged) &&
		{
			echo "100644 $O 1	unmerged" &&
			echo "100644 $A 2	unmerged" &&
			echo "100644 $B 3	unmerged"
		} | git update-index --index-info &&

		# Dirty common so there is something to restore
		echo dirty >>common &&

		# restore . without --ignore-unmerged should fail
		test_must_fail git restore . &&

		# restore --ignore-unmerged should succeed and restore common
		git restore --ignore-unmerged --quiet . >output 2>&1 &&
		test_must_be_empty output &&

		# common should be restored to its original content
		test "$(cat common)" = "one"
	)
'

test_expect_success 'restore a file on worktree from another ref (explicit paths)' '
	cd repo &&
	git reset --hard &&
	git show first:first.t >expected &&
	git restore --source=first first.t &&
	test_cmp expected first.t &&
	# Index should still have HEAD content
	HEAD_BLOB=$(git rev-parse HEAD:first.t) &&
	ACTUAL_INDEX_OID=$(git ls-files -s first.t | awk "{print \$2}") &&
	test "$ACTUAL_INDEX_OID" = "$HEAD_BLOB" &&
	git restore first.t
'

test_expect_success 'restore a file in both the index and worktree from another ref (round trip)' '
	cd repo &&
	git reset --hard &&
	FIRST_BLOB=$(git rev-parse first:first.t) &&
	git cat-file -p "$FIRST_BLOB" >expected_first &&
	git restore --source=first --staged --worktree first.t &&
	# Verify index
	ACTUAL_INDEX_OID=$(git ls-files -s first.t | awk "{print \$2}") &&
	test "$ACTUAL_INDEX_OID" = "$FIRST_BLOB" &&
	# Verify worktree
	test_cmp expected_first first.t &&
	# Restore both back to HEAD
	git restore --source=HEAD --staged --worktree first.t
'

test_expect_success 'restore a single file from HEAD when multiple files exist' '
	cd repo &&
	git reset --hard &&
	echo dirty >>one &&
	echo dirty >>two &&
	git restore one &&
	# one should be restored, two should still be dirty
	test "$(cat one)" = "one" &&
	grep dirty two &&
	git restore two
'

test_expect_success 'restore does not touch untracked files' '
	cd repo &&
	git reset --hard &&
	echo brand-new >brand-new &&
	git restore . &&
	# Untracked file should still exist
	test -f brand-new &&
	test "$(cat brand-new)" = "brand-new" &&
	rm brand-new
'

test_expect_success 'restore --staged with multiple files' '
	cd repo &&
	git reset --hard &&
	echo dirty1 >>one &&
	echo dirty2 >>two &&
	git add one two &&
	git restore --staged one two &&
	# Index should match HEAD for both files
	HEAD_ONE=$(git rev-parse HEAD:one) &&
	HEAD_TWO=$(git rev-parse HEAD:two) &&
	IDX_ONE=$(git ls-files -s one | awk "{print \$2}") &&
	IDX_TWO=$(git ls-files -s two | awk "{print \$2}") &&
	test "$IDX_ONE" = "$HEAD_ONE" &&
	test "$IDX_TWO" = "$HEAD_TWO" &&
	# But worktree should still be dirty
	grep dirty1 one &&
	grep dirty2 two &&
	git restore one two
'

test_expect_success 'restore --source with tag name' '
	cd repo &&
	git reset --hard &&
	FIRST_BLOB=$(git rev-parse first:first.t) &&
	git cat-file -p "$FIRST_BLOB" >expected &&
	git restore --source=first --worktree first.t &&
	test_cmp expected first.t &&
	git restore first.t
'

test_expect_success 'restore --staged followed by diff-index shows no staged changes' '
	cd repo &&
	git reset --hard &&
	echo new-content >>one &&
	git add one &&
	git restore --staged one &&
	git diff-index --cached --exit-code HEAD
'

# ---------------------------------------------------------------------------
# Additional restore tests
# ---------------------------------------------------------------------------

test_expect_success 'restore --source with commit SHA' '
	cd repo &&
	git reset --hard &&
	FIRST_SHA=$(git rev-parse first) &&
	FIRST_BLOB=$(git rev-parse first:first.t) &&
	git cat-file -p "$FIRST_BLOB" >expected &&
	git restore --source="$FIRST_SHA" --worktree first.t &&
	test_cmp expected first.t &&
	git restore first.t
'

test_expect_success 'restore --source --staged restores index to older state' '
	cd repo &&
	git reset --hard &&
	FIRST_BLOB=$(git rev-parse first:first.t) &&
	git restore --source=first --staged first.t &&
	ACTUAL=$(git ls-files -s first.t | awk "{print \$2}") &&
	test "$ACTUAL" = "$FIRST_BLOB" &&
	git restore --staged first.t
'

test_expect_success 'restore --source --staged --worktree restores both' '
	cd repo &&
	git reset --hard &&
	FIRST_BLOB=$(git rev-parse first:first.t) &&
	git cat-file -p "$FIRST_BLOB" >expected &&
	git restore --source=first --staged --worktree first.t &&
	ACTUAL=$(git ls-files -s first.t | awk "{print \$2}") &&
	test "$ACTUAL" = "$FIRST_BLOB" &&
	test_cmp expected first.t &&
	git restore --source=HEAD --staged --worktree first.t
'

test_expect_success 'restore worktree only does not change index' '
	cd repo &&
	git reset --hard &&
	HEAD_BLOB=$(git rev-parse HEAD:one) &&
	echo dirty >one &&
	git add one &&
	DIRTY_BLOB=$(git ls-files -s one | awk "{print \$2}") &&
	git restore --worktree one &&
	# Index should still have the dirty blob
	ACTUAL=$(git ls-files -s one | awk "{print \$2}") &&
	test "$ACTUAL" = "$DIRTY_BLOB" &&
	git restore --staged one
'

test_expect_success 'restore --staged file that was deleted from worktree' '
	cd repo &&
	git reset --hard &&
	rm one &&
	git add one &&
	git restore --staged one &&
	# Index should now match HEAD (file present)
	HEAD_BLOB=$(git rev-parse HEAD:one) &&
	ACTUAL=$(git ls-files -s one | awk "{print \$2}") &&
	test "$ACTUAL" = "$HEAD_BLOB" &&
	# Worktree still has file missing
	test_path_is_missing one &&
	git restore one
'

test_expect_success 'restore multiple files from worktree' '
	cd repo &&
	git reset --hard &&
	echo dirty1 >one &&
	echo dirty2 >two &&
	git restore one two &&
	test "$(cat one)" = "one" &&
	test "$(cat two)" = "two"
'

test_expect_success 'restore dot restores all modified tracked files' '
	cd repo &&
	git reset --hard &&
	echo dirty1 >one &&
	echo dirty2 >two &&
	git restore . &&
	test "$(cat one)" = "one" &&
	test "$(cat two)" = "two"
'

test_expect_success 'restore --quiet suppresses output' '
	cd repo &&
	git reset --hard &&
	echo dirty >one &&
	git restore --quiet one >stdout 2>stderr &&
	test_must_be_empty stdout &&
	test_must_be_empty stderr &&
	test "$(cat one)" = "one"
'

test_expect_success 'restore nonexistent pathspec fails' '
	cd repo &&
	git reset --hard &&
	test_must_fail git restore nonexistent-file
'

test_expect_success 'restore --source nonexistent ref fails' '
	cd repo &&
	git reset --hard &&
	test_must_fail git restore --source=nonexistent-ref first.t
'

test_expect_success 'restore --staged then commit does not include unstaged file' '
	cd repo &&
	git reset --hard &&
	echo changed >one &&
	echo changed >two &&
	git add one two &&
	git restore --staged one &&
	git commit -m "only two" &&
	# one should not be in the commit
	git diff-tree --no-commit-id --name-only -r HEAD >committed &&
	grep two committed &&
	test_must_fail grep one committed &&
	git reset --hard HEAD~1
'

test_expect_success 'restore --worktree is default when no flags given' '
	cd repo &&
	git reset --hard &&
	echo dirty >one &&
	git restore one &&
	test "$(cat one)" = "one"
'

test_done
