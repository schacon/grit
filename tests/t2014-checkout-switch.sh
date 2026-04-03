#!/bin/sh
#
# Tests for 'grit checkout' — switching branches (no -b), with dirty worktree.

test_description='grit checkout branch switching with dirty worktree'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository with branches' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "base" >common.txt &&
	echo "master-only" >master-file.txt &&
	mkdir -p dir &&
	echo "dir-content" >dir/file.txt &&
	git add . &&
	git commit -m "initial on master" &&

	git checkout -b branchA &&
	echo "branchA" >common.txt &&
	echo "A-only" >a-file.txt &&
	git add . &&
	git commit -m "commit on branchA" &&

	git checkout master &&
	git checkout -b branchB &&
	echo "branchB" >common.txt &&
	echo "B-only" >b-file.txt &&
	git add . &&
	git commit -m "commit on branchB" &&

	git checkout master
'

# ---------------------------------------------------------------------------
# Basic switching (no dirty worktree)
# ---------------------------------------------------------------------------
test_expect_success 'switch to branchA' '
	cd repo &&
	git checkout branchA &&
	test "$(git symbolic-ref --short HEAD)" = "branchA" &&
	test "$(cat common.txt)" = "branchA" &&
	git checkout master
'

test_expect_success 'switch to branchB' '
	cd repo &&
	git checkout branchB &&
	test "$(git symbolic-ref --short HEAD)" = "branchB" &&
	test "$(cat common.txt)" = "branchB" &&
	git checkout master
'

test_expect_success 'switch back and forth' '
	cd repo &&
	git checkout branchA &&
	git checkout branchB &&
	git checkout branchA &&
	test "$(git symbolic-ref --short HEAD)" = "branchA" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Dirty worktree: untracked files (should be fine)
# ---------------------------------------------------------------------------
test_expect_success 'switch with untracked file succeeds' '
	cd repo &&
	echo "untracked" >untracked.txt &&
	git checkout branchA &&
	test -f untracked.txt &&
	test "$(git symbolic-ref --short HEAD)" = "branchA" &&
	git checkout master &&
	rm -f untracked.txt
'

test_expect_success 'switch with untracked dir succeeds' '
	cd repo &&
	mkdir -p untracked-dir &&
	echo "uf" >untracked-dir/file &&
	git checkout branchA &&
	test -f untracked-dir/file &&
	git checkout master &&
	rm -rf untracked-dir
'

# ---------------------------------------------------------------------------
# Dirty worktree: compatible changes (not conflicting)
# ---------------------------------------------------------------------------
test_expect_success 'switch with dirty file not conflicting succeeds' '
	cd repo &&
	echo "dirty-master-file" >master-file.txt &&
	git checkout branchA &&
	test "$(cat master-file.txt)" = "dirty-master-file" &&
	git checkout master &&
	git checkout -- master-file.txt
'

test_expect_failure 'switch with staged non-conflicting change succeeds' '
	cd repo &&
	echo "staged" >master-file.txt &&
	git add master-file.txt &&
	git checkout branchA &&
	test "$(cat master-file.txt)" = "staged" &&
	git checkout master &&
	git reset HEAD -- master-file.txt &&
	git checkout -- master-file.txt
'

# ---------------------------------------------------------------------------
# Dirty worktree: conflicting changes (should fail)
# ---------------------------------------------------------------------------
test_expect_failure 'switch refuses with conflicting dirty file' '
	cd repo &&
	echo "dirty-common" >common.txt &&
	test_must_fail git checkout branchA 2>err &&
	grep -i "overwritten" err &&
	git checkout -- common.txt
'

test_expect_failure 'switch refuses with conflicting staged file' '
	cd repo &&
	echo "staged-common" >common.txt &&
	git add common.txt &&
	test_must_fail git checkout branchA 2>err &&
	grep -i "overwritten" err &&
	git reset HEAD -- common.txt &&
	git checkout -- common.txt
'

# ---------------------------------------------------------------------------
# HEAD and index are unchanged after refused switch
# ---------------------------------------------------------------------------
test_expect_failure 'HEAD unchanged after refused switch' '
	cd repo &&
	head_before=$(git rev-parse HEAD) &&
	echo "dirty-common" >common.txt &&
	test_must_fail git checkout branchA 2>/dev/null &&
	head_after=$(git rev-parse HEAD) &&
	test "$head_before" = "$head_after" &&
	test "$(git symbolic-ref --short HEAD)" = "master" &&
	git checkout -- common.txt
'

# ---------------------------------------------------------------------------
# Switch with new file that would conflict with branch file
# ---------------------------------------------------------------------------
test_expect_failure 'switch refuses when new file conflicts with branch file' '
	cd repo &&
	echo "local-a-file" >a-file.txt &&
	git add a-file.txt &&
	test_must_fail git checkout branchA 2>err &&
	test -s err &&
	git reset HEAD -- a-file.txt &&
	rm -f a-file.txt
'

# ---------------------------------------------------------------------------
# Switch between branches with different directory structures
# ---------------------------------------------------------------------------
test_expect_failure 'setup branches with different dirs' '
	cd repo &&
	git checkout master &&
	git checkout -b has-subdir &&
	mkdir -p extra/nested &&
	echo "nested" >extra/nested/file.txt &&
	git add . &&
	git commit -m "add nested dir" &&
	git checkout master
'

test_expect_failure 'switch to branch with extra dirs adds them' '
	cd repo &&
	git checkout has-subdir &&
	test -f extra/nested/file.txt &&
	test "$(cat extra/nested/file.txt)" = "nested" &&
	git checkout master
'

test_expect_failure 'switch from branch removes extra dirs' '
	cd repo &&
	git checkout has-subdir &&
	git checkout master &&
	test_path_is_missing extra/nested/file.txt
'

# ---------------------------------------------------------------------------
# Switch with deleted file in worktree
# ---------------------------------------------------------------------------
test_expect_failure 'switch with deleted tracked file restores on return' '
	cd repo &&
	rm -f master-file.txt &&
	git checkout branchA &&
	git checkout -f master &&
	test -f master-file.txt &&
	test "$(cat master-file.txt)" = "master-only"
'

# ---------------------------------------------------------------------------
# Switch preserves staged changes on non-conflicting files
# ---------------------------------------------------------------------------
test_expect_failure 'switch preserves staged new file' '
	cd repo &&
	echo "new-staged" >new-staged.txt &&
	git add new-staged.txt &&
	git checkout branchA &&
	git diff --cached --name-only >staged-list &&
	grep new-staged.txt staged-list &&
	git checkout master &&
	git reset HEAD -- new-staged.txt &&
	rm -f new-staged.txt
'

# ---------------------------------------------------------------------------
# Switch with multiple dirty files, one conflicting
# ---------------------------------------------------------------------------
test_expect_failure 'one conflicting dirty file blocks entire switch' '
	cd repo &&
	echo "dirty-master" >master-file.txt &&
	echo "dirty-common" >common.txt &&
	test_must_fail git checkout branchA 2>err &&
	test "$(git symbolic-ref --short HEAD)" = "master" &&
	git checkout -- common.txt &&
	git checkout -- master-file.txt
'

# ---------------------------------------------------------------------------
# Switch to same branch is a no-op
# ---------------------------------------------------------------------------
test_expect_failure 'checkout same branch is a no-op' '
	cd repo &&
	git checkout master 2>err &&
	test "$(git symbolic-ref --short HEAD)" = "master"
'

# ---------------------------------------------------------------------------
# Switch using - (previous branch)
# ---------------------------------------------------------------------------
test_expect_failure 'checkout - switches to previous branch' '
	cd repo &&
	git checkout branchA &&
	git checkout master &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "branchA" &&
	git checkout master
'

test_expect_failure 'checkout - alternates correctly' '
	cd repo &&
	git checkout branchA &&
	git checkout branchB &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "branchA" &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "branchB" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Switch to nonexistent branch fails
# ---------------------------------------------------------------------------
test_expect_failure 'checkout nonexistent branch fails' '
	cd repo &&
	test_must_fail git checkout no-such-branch 2>err &&
	test -s err &&
	test "$(git symbolic-ref --short HEAD)" = "master"
'

# ---------------------------------------------------------------------------
# Switch with dirty worktree file that matches target (trivial merge)
# ---------------------------------------------------------------------------
test_expect_failure 'switch refuses even when dirty file matches target' '
	cd repo &&
	git checkout master &&
	echo "branchA" >common.txt &&
	test_must_fail git checkout branchA 2>err &&
	test "$(git symbolic-ref --short HEAD)" = "master" &&
	git checkout -- common.txt
'

# ---------------------------------------------------------------------------
# Switch with permission bits preserved
# ---------------------------------------------------------------------------
test_expect_failure 'switch with only worktree-deleted file carries deletion' '
	cd repo &&
	git checkout master &&
	rm -f dir/file.txt &&
	git checkout branchA &&
	git checkout -f master &&
	test -f dir/file.txt
'

test_done
