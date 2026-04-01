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

test_done
