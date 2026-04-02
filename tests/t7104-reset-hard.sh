#!/bin/sh
#
# Tests for 'reset --hard' edge cases: unmerged entries, index corruption
# checks, and various dirty state recovery.
# Adapted from git/t/t7104-reset-hard.sh

test_description='reset --hard edge cases'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup: repo with directories and files
# ---------------------------------------------------------------------------
test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	mkdir before &&
	mkdir later &&
	echo 1 >before/1 &&
	echo 2 >before/2 &&
	echo hello >hello &&
	echo 3 >later/3 &&
	git add before hello later &&
	git commit -m "initial" &&
	git write-tree >../init_tree
'

# ---------------------------------------------------------------------------
# Simulate unmerged entry: put hello at stage 2, then reset --hard
# ---------------------------------------------------------------------------
test_expect_success 'setup unmerged entry' '
	cd repo &&
	H=$(git rev-parse HEAD:hello) &&
	git rm --cached hello &&
	echo "100644 $H 2	hello" | git update-index --index-info &&
	# Verify unmerged entries exist
	git ls-files -u >unmerged &&
	grep hello unmerged
'

test_expect_success 'reset --hard should restore unmerged ones' '
	cd repo &&
	git reset --hard &&
	git ls-files --error-unmatch before/1 before/2 hello later/3 &&
	test -f hello &&
	test "$(cat hello)" = "hello"
'

test_expect_success 'reset --hard did not corrupt index or cache-tree' '
	cd repo &&
	T=$(git write-tree) &&
	rm -f .git/index &&
	git add before hello later &&
	U=$(git write-tree) &&
	test "$T" = "$U"
'

# ---------------------------------------------------------------------------
# reset --hard removes added files not in target
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard removes added files not in target' '
	cd repo &&
	git reset --hard &&
	echo brand-new >brand-new &&
	git add brand-new &&
	git reset --hard &&
	test_path_is_missing brand-new
'

# ---------------------------------------------------------------------------
# reset --hard restores deleted tracked files
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard restores deleted tracked files' '
	cd repo &&
	git reset --hard &&
	rm hello &&
	test_path_is_missing hello &&
	git reset --hard &&
	test -f hello &&
	test "$(cat hello)" = "hello"
'

# ---------------------------------------------------------------------------
# reset --hard restores modified tracked files
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard restores modified tracked files' '
	cd repo &&
	git reset --hard &&
	echo dirty >hello &&
	echo dirty >before/1 &&
	git reset --hard &&
	test "$(cat hello)" = "hello" &&
	test "$(cat before/1)" = "1"
'

# ---------------------------------------------------------------------------
# reset --hard with all files deleted restores everything
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard with all tracked files deleted' '
	cd repo &&
	git reset --hard &&
	rm hello before/1 before/2 later/3 &&
	git reset --hard &&
	test -f hello &&
	test -f before/1 &&
	test -f before/2 &&
	test -f later/3
'

# ---------------------------------------------------------------------------
# write-tree matches after --hard reset from messy state
# ---------------------------------------------------------------------------
test_expect_success 'write-tree matches after --hard reset from messy state' '
	cd repo &&
	init_tree=$(cat ../init_tree) &&

	# Make a mess: staged changes and extra files
	echo dirty >hello &&
	echo extra >extra &&
	git add hello extra &&

	git reset --hard &&
	actual_tree=$(git write-tree) &&
	test "$actual_tree" = "$init_tree"
'

# ---------------------------------------------------------------------------
# reset --hard to a different commit
# ---------------------------------------------------------------------------
test_expect_success 'setup second commit' '
	cd repo &&
	git reset --hard &&
	echo modified >hello &&
	echo 4 >later/4 &&
	git add hello later/4 &&
	git commit -m "second" &&
	git rev-parse HEAD >../second_oid
'

test_expect_success 'reset --hard to earlier commit removes new files' '
	cd repo &&
	second=$(cat ../second_oid) &&
	init_tree=$(cat ../init_tree) &&
	git reset --hard HEAD^ &&
	test "$(cat hello)" = "hello" &&
	test_path_is_missing later/4 &&
	actual_tree=$(git write-tree) &&
	test "$actual_tree" = "$init_tree"
'

test_expect_success 'reset --hard back to later commit restores files' '
	cd repo &&
	second=$(cat ../second_oid) &&
	git reset --hard "$second" &&
	test "$(cat hello)" = "modified" &&
	test -f later/4 &&
	test "$(cat later/4)" = "4"
'

# ---------------------------------------------------------------------------
# reset --hard with staged+unstaged mix
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard discards both staged and unstaged changes' '
	cd repo &&
	git reset --hard &&
	echo staged >hello &&
	git add hello &&
	echo unstaged >hello &&
	git reset --hard &&
	test "$(cat hello)" = "modified"
'

# ---------------------------------------------------------------------------
# reset --hard prints message
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard prints HEAD is now at message' '
	cd repo &&
	git reset --hard HEAD >.actual &&
	head_hex=$(git rev-parse --short HEAD) &&
	echo "HEAD is now at $head_hex second" >.expected &&
	test_cmp .expected .actual
'

# ---------------------------------------------------------------------------
# reset --hard --quiet suppresses output
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard --quiet suppresses output' '
	cd repo &&
	git reset --hard --quiet HEAD >stdout 2>stderr &&
	test_must_be_empty stdout &&
	test_must_be_empty stderr
'

# ---------------------------------------------------------------------------
# reset --hard preserves untracked files
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard preserves untracked files' '
	cd repo &&
	git reset --hard &&
	echo untracked >untracked-file &&
	git reset --hard &&
	test -f untracked-file &&
	rm -f untracked-file
'

# ---------------------------------------------------------------------------
# reset --hard with empty directory removal
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard removes directories that become empty' '
	cd repo &&
	git reset --hard &&
	mkdir -p newdir &&
	echo new >newdir/file &&
	git add newdir/file &&
	git reset --hard &&
	test_path_is_missing newdir/file
'

# ---------------------------------------------------------------------------
# reset --hard HEAD is idempotent
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard HEAD is idempotent' '
	cd repo &&
	git reset --hard &&
	tree1=$(git write-tree) &&
	git reset --hard HEAD &&
	tree2=$(git write-tree) &&
	test "$tree1" = "$tree2"
'

# ---------------------------------------------------------------------------
# reset --hard with new file committed then reset to parent
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard to parent removes committed new file' '
	cd repo &&
	git reset --hard $(cat ../second_oid) &&
	echo brand >brand-new2 &&
	git add brand-new2 &&
	git commit -m "add brand-new2" &&
	test -f brand-new2 &&
	git reset --hard HEAD~1 &&
	test_path_is_missing brand-new2
'

# ---------------------------------------------------------------------------
# reset --hard with permission-changed file
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard restores file content after chmod change' '
	cd repo &&
	git reset --hard $(cat ../second_oid) &&
	chmod 755 hello &&
	echo extra >>hello &&
	git reset --hard &&
	test "$(cat hello)" = "modified"
'

# ---------------------------------------------------------------------------
# reset --hard after rm -rf of subdirectory
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard after rm -rf of subdirectory' '
	cd repo &&
	git reset --hard $(cat ../second_oid) &&
	rm -rf before &&
	test_path_is_missing before/1 &&
	git reset --hard &&
	test -f before/1 &&
	test -f before/2
'

# ---------------------------------------------------------------------------
# reset --hard with mixed staged/unstaged in subdirectory
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard with staged changes in subdirectory' '
	cd repo &&
	git reset --hard $(cat ../second_oid) &&
	echo dirty >before/1 &&
	git add before/1 &&
	echo dirtier >before/1 &&
	git reset --hard &&
	test "$(cat before/1)" = "1"
'

# ---------------------------------------------------------------------------
# reset --hard does not affect other branches
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard does not modify other branch refs' '
	cd repo &&
	git reset --hard $(cat ../second_oid) &&
	git branch side-branch &&
	side_oid=$(git rev-parse side-branch) &&
	git reset --hard HEAD~1 &&
	test "$(git rev-parse side-branch)" = "$side_oid" &&
	git branch -D side-branch
'

# ---------------------------------------------------------------------------
# reset --hard with symlink-like content
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard restores after all files modified' '
	cd repo &&
	git reset --hard $(cat ../second_oid) &&
	for f in hello before/1 before/2 later/3 later/4; do
		echo DIRTY >"$f" || return 1
	done &&
	git reset --hard &&
	test "$(cat hello)" = "modified" &&
	test "$(cat before/1)" = "1" &&
	test "$(cat later/4)" = "4"
'

# ---------------------------------------------------------------------------
# reset --hard with --quiet and dirty state
# ---------------------------------------------------------------------------
test_expect_success 'reset --hard --quiet with dirty state still cleans' '
	cd repo &&
	git reset --hard $(cat ../second_oid) &&
	echo dirty >hello &&
	git add hello &&
	git reset --hard --quiet >stdout 2>stderr &&
	test_must_be_empty stdout &&
	test_must_be_empty stderr &&
	test "$(cat hello)" = "modified"
'

test_done
