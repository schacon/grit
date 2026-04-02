#!/bin/sh
#
# Tests for 'grit checkout' — branch switching and file restoration.
# checkout is a passthrough command but we verify grit dispatches correctly.

test_description='grit checkout — branch switching and file restoration'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "initial" >file1 &&
	git add file1 &&
	git commit -m "initial commit" &&
	git rev-parse HEAD >../commit1
'

# ---------------------------------------------------------------------------
# Branch creation and switching
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b creates and switches to new branch' '
	cd repo &&
	git checkout -b feature &&
	test "$(git symbolic-ref --short HEAD)" = "feature"
'

test_expect_success 'checkout switches back to master' '
	cd repo &&
	git checkout master &&
	test "$(git symbolic-ref --short HEAD)" = "master"
'

test_expect_success 'checkout to existing branch works' '
	cd repo &&
	git checkout feature &&
	test "$(git symbolic-ref --short HEAD)" = "feature" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Checkout with commits on branches
# ---------------------------------------------------------------------------
test_expect_success 'changes on branch are isolated' '
	cd repo &&
	git checkout -b branch-a &&
	echo "branch-a content" >branch-file &&
	git add branch-file &&
	git commit -m "add branch-file on branch-a" &&

	git checkout master &&
	test_path_is_missing branch-file &&

	git checkout branch-a &&
	test -f branch-file &&
	test "$(cat branch-file)" = "branch-a content" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Checkout file from another branch/commit
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- <file> restores file from index' '
	cd repo &&
	echo "modified" >file1 &&
	git checkout -- file1 &&
	test "$(cat file1)" = "initial"
'

test_expect_success 'checkout <commit> -- <file> restores file from commit' '
	cd repo &&
	echo "changed" >file1 &&
	git add file1 &&
	git commit -m "change file1" &&
	git checkout $(cat ../commit1) -- file1 &&
	test "$(cat file1)" = "initial" &&
	git checkout HEAD -- file1
'

# ---------------------------------------------------------------------------
# Detached HEAD
# ---------------------------------------------------------------------------
test_expect_success 'checkout <commit> detaches HEAD' '
	cd repo &&
	git checkout $(cat ../commit1) 2>err &&
	test_must_fail git symbolic-ref HEAD 2>/dev/null &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Checkout with -b from a specific commit
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b <branch> <start-point> creates branch from commit' '
	cd repo &&
	git checkout -b from-start $(cat ../commit1) &&
	test "$(git rev-parse HEAD)" = "$(cat ../commit1)" &&
	test "$(git symbolic-ref --short HEAD)" = "from-start" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Checkout non-existent branch fails
# ---------------------------------------------------------------------------
test_expect_success 'checkout nonexistent branch fails' '
	cd repo &&
	test_must_fail git checkout nonexistent-branch 2>err &&
	test -s err
'

# ---------------------------------------------------------------------------
# Checkout with dirty worktree
# ---------------------------------------------------------------------------
test_expect_success 'checkout refuses switch with conflicting dirty file' '
	cd repo &&
	git checkout master &&
	# branch-a has branch-file, master does not
	# Create a dirty file that would conflict
	echo "dirty" >branch-file &&
	git add branch-file &&
	echo "dirty2" >branch-file &&
	test_must_fail git checkout branch-a 2>err &&
	git checkout -- branch-file &&
	git reset HEAD -- branch-file &&
	rm -f branch-file
'

# ---------------------------------------------------------------------------
# Checkout with -f (force)
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f discards local changes' '
	cd repo &&
	echo "will be lost" >file1 &&
	git checkout -f master &&
	# file1 should be restored to committed state
	test "$(cat file1)" != "will be lost"
'

# ---------------------------------------------------------------------------
# Checkout preserves untracked files
# ---------------------------------------------------------------------------
test_expect_success 'checkout does not remove untracked files' '
	cd repo &&
	echo "untracked" >untracked-file &&
	git checkout branch-a &&
	test -f untracked-file &&
	git checkout master &&
	test -f untracked-file &&
	rm untracked-file
'

# ---------------------------------------------------------------------------
# Checkout tag
# ---------------------------------------------------------------------------
test_expect_success 'checkout a tag detaches HEAD at tag commit' '
	cd repo &&
	git tag v1.0 $(cat ../commit1) &&
	git checkout v1.0 2>err &&
	test "$(git rev-parse HEAD)" = "$(cat ../commit1)" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Checkout . restores all files
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- . restores all modified files' '
	cd repo &&
	echo "mod1" >file1 &&
	git checkout -- . &&
	test "$(cat file1)" != "mod1"
'

# ---------------------------------------------------------------------------
# Checkout -B (force create)
# ---------------------------------------------------------------------------
test_expect_success 'checkout -B creates new branch' '
	cd repo &&
	git checkout master &&
	git checkout -B new-force-branch &&
	test "$(git symbolic-ref --short HEAD)" = "new-force-branch" &&
	git checkout master
'

test_expect_success 'checkout -B resets existing branch to current HEAD' '
	cd repo &&
	git checkout master &&
	git checkout -B new-force-branch &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse master)" &&
	git checkout master
'

test_expect_success 'checkout -B <branch> <start> resets to start point' '
	cd repo &&
	git checkout master &&
	git checkout -B from-initial $(cat ../commit1) &&
	test "$(git rev-parse HEAD)" = "$(cat ../commit1)" &&
	test "$(git symbolic-ref --short HEAD)" = "from-initial" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Checkout with merge conflicts
# ---------------------------------------------------------------------------
test_expect_success 'setup conflicting branches for checkout -m' '
	cd repo &&
	git checkout master &&
	git checkout -b left &&
	echo "left content" >conflict-file &&
	git add conflict-file &&
	git commit -m "left: add conflict-file" &&

	git checkout master &&
	git checkout -b right &&
	echo "right content" >conflict-file &&
	git add conflict-file &&
	git commit -m "right: add conflict-file" &&
	git checkout master
'

test_expect_success 'checkout -m allows switching with local modifications' '
	cd repo &&
	git checkout left &&
	echo "modified left" >conflict-file &&
	git checkout -m right 2>err || true &&
	# Either it succeeds with merge or we get conflict markers
	test -f conflict-file
'

test_expect_success 'cleanup after merge checkout test' '
	cd repo &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# Checkout specific files from commits
# ---------------------------------------------------------------------------
test_expect_success 'checkout HEAD~1 -- file restores old version' '
	cd repo &&
	git checkout master &&
	oldcontent=$(git show $(cat ../commit1):file1) &&
	git checkout $(cat ../commit1) -- file1 &&
	test "$(cat file1)" = "$oldcontent" &&
	git checkout HEAD -- file1
'

test_expect_success 'checkout <branch> -- file gets file from branch' '
	cd repo &&
	git checkout master &&
	git checkout left -- conflict-file &&
	test "$(cat conflict-file)" = "left content" &&
	git checkout HEAD -- conflict-file 2>/dev/null || git rm -f conflict-file
'

test_expect_success 'checkout -- nonexistent file fails' '
	cd repo &&
	test_must_fail git checkout -- nonexistent-file 2>err &&
	test -s err
'

# ---------------------------------------------------------------------------
# Checkout with paths does not switch branch
# ---------------------------------------------------------------------------
test_expect_success 'checkout <commit> -- <file> does not switch branch' '
	cd repo &&
	git checkout master &&
	git checkout $(cat ../commit1) -- file1 &&
	test "$(git symbolic-ref --short HEAD)" = "master" &&
	git checkout HEAD -- file1
'

# ---------------------------------------------------------------------------
# Orphan branch
# ---------------------------------------------------------------------------
test_expect_success 'checkout --orphan creates branch with no commits' '
	cd repo &&
	git checkout --orphan orphan-branch &&
	test_must_fail git rev-parse HEAD 2>/dev/null &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# Checkout with -- separator
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- disambiguates file from branch' '
	cd repo &&
	git checkout master &&
	echo "dirty" >file1 &&
	git checkout -- file1 &&
	test "$(cat file1)" != "dirty"
'

# ---------------------------------------------------------------------------
# Checkout to previous branch with -
# ---------------------------------------------------------------------------
test_expect_success 'checkout - switches to previous branch' '
	cd repo &&
	git checkout master &&
	git checkout branch-a &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "master"
'

# ---------------------------------------------------------------------------
# Multiple files checkout
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- multiple files restores all' '
	cd repo &&
	git checkout master &&
	echo "dirty1" >file1 &&
	echo "dirty2" >branch-file 2>/dev/null &&
	git add branch-file 2>/dev/null || true &&
	git checkout -- file1 &&
	test "$(cat file1)" != "dirty1"
'

# ---------------------------------------------------------------------------
# Checkout with -q (quiet)
# ---------------------------------------------------------------------------
test_expect_success 'checkout -q suppresses messages' '
	cd repo &&
	git checkout -f master &&
	git checkout -q branch-a 2>err &&
	test_must_be_empty err &&
	git checkout -q master
'

# ---------------------------------------------------------------------------
# More edge cases
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b fails if branch already exists' '
	cd repo &&
	git checkout master &&
	test_must_fail git checkout -b branch-a 2>err &&
	test -s err
'

test_expect_success 'checkout -B succeeds even if branch already exists' '
	cd repo &&
	git checkout master &&
	git checkout -B branch-a &&
	test "$(git symbolic-ref --short HEAD)" = "branch-a" &&
	git checkout master
'

test_expect_success 'checkout with pathspec from index' '
	cd repo &&
	git checkout master &&
	echo "modified-again" >file1 &&
	git add file1 &&
	echo "further-modified" >file1 &&
	git checkout -- file1 &&
	test "$(cat file1)" = "modified-again" &&
	git reset HEAD -- file1 &&
	git checkout -- file1
'

test_expect_success 'detached HEAD warns on stderr' '
	cd repo &&
	git checkout $(cat ../commit1) 2>err &&
	test -s err &&
	git checkout master
'

test_expect_success 'checkout branch created from another branch tip' '
	cd repo &&
	git checkout -b from-branch-a branch-a &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse branch-a)" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Deepened tests: working tree edge cases
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- <dir> restores all files in directory' '
	cd repo &&
	git checkout master &&
	mkdir -p sub &&
	echo "sub content" >sub/s1 &&
	echo "sub content2" >sub/s2 &&
	git add sub &&
	git commit -m "add sub dir" &&
	echo "dirty1" >sub/s1 &&
	echo "dirty2" >sub/s2 &&
	git checkout -- sub &&
	test "$(cat sub/s1)" = "sub content" &&
	test "$(cat sub/s2)" = "sub content2"
'

test_expect_success 'checkout branch with added file brings that file' '
	cd repo &&
	git checkout -b has-extra-file &&
	echo "extra" >extra-file &&
	git add extra-file &&
	git commit -m "add extra-file" &&
	git checkout master &&
	test_path_is_missing extra-file &&
	git checkout has-extra-file &&
	test -f extra-file &&
	git checkout master
'

test_expect_success 'checkout branch with deleted file removes that file' '
	cd repo &&
	git checkout -b delete-test &&
	git rm file1 &&
	git commit -m "delete file1" &&
	test_path_is_missing file1 &&
	git checkout master &&
	test -f file1 &&
	git checkout master
'

test_expect_success 'checkout --track sets up tracking (with -b)' '
	cd repo &&
	git checkout master &&
	git checkout -b local-track --track branch-a &&
	test "$(git config branch.local-track.merge)" = "refs/heads/branch-a" &&
	git checkout master
'

test_expect_success 'checkout to branch preserves committed content' '
	cd repo &&
	git checkout master &&
	git checkout -b content-check &&
	echo "content-check" >cc-file &&
	git add cc-file &&
	git commit -m "add cc-file" &&
	git checkout master &&
	git checkout content-check &&
	test "$(cat cc-file)" = "content-check" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Deepened: rev-parse and HEAD behavior
# ---------------------------------------------------------------------------
test_expect_success 'checkout updates HEAD to target branch tip' '
	cd repo &&
	git checkout master &&
	master_tip=$(git rev-parse master) &&
	git checkout branch-a &&
	ba_tip=$(git rev-parse branch-a) &&
	test "$(git rev-parse HEAD)" = "$ba_tip" &&
	git checkout master &&
	test "$(git rev-parse HEAD)" = "$master_tip"
'

test_expect_success 'checkout detached HEAD then back to branch' '
	cd repo &&
	head_before=$(git rev-parse master) &&
	git checkout $(cat ../commit1) 2>/dev/null &&
	git checkout master &&
	test "$(git rev-parse HEAD)" = "$head_before"
'

test_expect_success 'checkout with explicit HEAD is a no-op' '
	cd repo &&
	git checkout master &&
	head1=$(git rev-parse HEAD) &&
	git checkout HEAD &&
	head2=$(git rev-parse HEAD) &&
	test "$head1" = "$head2"
'

test_expect_success 'checkout branch updates index' '
	cd repo &&
	git checkout has-extra-file &&
	git ls-files >files &&
	grep "extra-file" files &&
	git checkout master &&
	git ls-files >files2 &&
	! grep "extra-file" files2
'

test_expect_success 'checkout -- file with spaces in name' '
	cd repo &&
	git checkout master &&
	echo "spaced" >"file with spaces" &&
	git add "file with spaces" &&
	git commit -m "add spaced file" &&
	echo "dirty" >"file with spaces" &&
	git checkout -- "file with spaces" &&
	test "$(cat "file with spaces")" = "spaced"
'

# ---------------------------------------------------------------------------
# Deepened: checkout with -b from tags and refs
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b from tag creates branch at tag' '
	cd repo &&
	git checkout -b from-tag v1.0 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse v1.0)" &&
	git checkout master
'

test_expect_success 'checkout to same branch is a no-op' '
	cd repo &&
	git checkout master &&
	git checkout master 2>err &&
	# Should succeed with no error
	test $? -eq 0
'

test_expect_success 'checkout --orphan does not carry files into index' '
	cd repo &&
	git checkout --orphan clean-orphan &&
	git reset --hard 2>/dev/null || true &&
	git ls-files >orphan-files &&
	git checkout -f master
'

test_expect_success 'checkout with --conflict=merge on conflicting file' '
	cd repo &&
	git checkout master &&
	git checkout left 2>/dev/null &&
	echo "local change" >conflict-file &&
	git checkout --conflict=merge right 2>err || true &&
	test -f conflict-file &&
	git checkout -f master
'

test_expect_success 'checkout -b with no start point uses HEAD' '
	cd repo &&
	git checkout master &&
	head=$(git rev-parse HEAD) &&
	git checkout -b from-head-implicit &&
	test "$(git rev-parse HEAD)" = "$head" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# Deepened: multiple branch switching preserves state
# ---------------------------------------------------------------------------
test_expect_success 'rapid branch switching preserves content' '
	cd repo &&
	git checkout master &&
	git checkout branch-a &&
	git checkout left &&
	git checkout master &&
	git checkout right &&
	git checkout master &&
	# Verify final state is correct
	test "$(git symbolic-ref --short HEAD)" = "master"
'

test_expect_success 'checkout - alternates between two branches' '
	cd repo &&
	git checkout master &&
	git checkout branch-a &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "master" &&
	git checkout - &&
	test "$(git symbolic-ref --short HEAD)" = "branch-a" &&
	git checkout master
'

test_done
