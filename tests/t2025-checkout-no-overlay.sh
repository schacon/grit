#!/bin/sh
#
# Tests for 'grit checkout --no-overlay' — removes files not in source.

test_description='grit checkout --no-overlay'

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

	echo "shared" >shared.txt &&
	echo "master-only" >master-only.txt &&
	mkdir -p dir &&
	echo "dir-shared" >dir/shared.txt &&
	echo "dir-master" >dir/master.txt &&
	git add . &&
	git commit -m "initial on master" &&

	git checkout -b other &&
	echo "shared-other" >shared.txt &&
	echo "other-only" >other-only.txt &&
	echo "dir-shared-other" >dir/shared.txt &&
	echo "dir-other" >dir/other.txt &&
	git rm master-only.txt &&
	git rm dir/master.txt &&
	git add . &&
	git commit -m "commit on other" &&

	git checkout master
'

# ---------------------------------------------------------------------------
# --no-overlay removes files not in source tree
# ---------------------------------------------------------------------------
test_expect_failure 'checkout --no-overlay other -- . removes master-only files' '
	cd repo &&
	git checkout --no-overlay other -- . &&
	test_path_is_missing master-only.txt &&
	test_path_is_missing dir/master.txt &&
	test -f shared.txt &&
	test "$(cat shared.txt)" = "shared-other" &&
	test -f other-only.txt &&
	git checkout master -- . &&
	git checkout master
'

test_expect_success 'regular checkout (overlay) keeps files not in source' '
	cd repo &&
	git checkout other -- . &&
	test -f master-only.txt &&
	test "$(cat shared.txt)" = "shared-other" &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay with specific directory
# ---------------------------------------------------------------------------
test_expect_failure 'checkout --no-overlay other -- dir/ removes dir-only-on-master' '
	cd repo &&
	git checkout --no-overlay other -- dir &&
	test_path_is_missing dir/master.txt &&
	test -f dir/shared.txt &&
	test "$(cat dir/shared.txt)" = "dir-shared-other" &&
	test -f dir/other.txt &&
	test -f master-only.txt &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay with single file (no removal effect)
# ---------------------------------------------------------------------------
test_expect_failure 'checkout --no-overlay other -- shared.txt updates file' '
	cd repo &&
	git checkout --no-overlay other -- shared.txt &&
	test "$(cat shared.txt)" = "shared-other" &&
	test -f master-only.txt &&
	git checkout master -- shared.txt
'

# ---------------------------------------------------------------------------
# --no-overlay removes from index too
# ---------------------------------------------------------------------------
test_expect_failure 'checkout --no-overlay removes files from index' '
	cd repo &&
	git checkout --no-overlay other -- . &&
	git ls-files >index-files &&
	! grep master-only.txt index-files &&
	! grep "dir/master.txt" index-files &&
	grep shared.txt index-files &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay with HEAD (restore to HEAD, removing staged additions)
# ---------------------------------------------------------------------------
test_expect_success 'setup extra staged file' '
	cd repo &&
	echo "extra" >extra-staged.txt &&
	git add extra-staged.txt
'

test_expect_failure 'checkout --no-overlay HEAD -- . removes staged-only files' '
	cd repo &&
	git checkout --no-overlay HEAD -- . &&
	test_path_is_missing extra-staged.txt &&
	git ls-files >index-files &&
	! grep extra-staged.txt index-files
'

# ---------------------------------------------------------------------------
# --no-overlay with branch that has fewer files
# ---------------------------------------------------------------------------
test_expect_success 'setup branch with minimal files' '
	cd repo &&
	git checkout -b minimal &&
	git rm shared.txt &&
	git rm dir/shared.txt &&
	git rm dir/master.txt &&
	git commit -m "minimal branch" &&
	git checkout master
'

test_expect_failure 'checkout --no-overlay minimal -- . removes most files' '
	cd repo &&
	git checkout --no-overlay minimal -- . &&
	test -f master-only.txt &&
	test_path_is_missing shared.txt &&
	test_path_is_missing dir/shared.txt &&
	test_path_is_missing dir/master.txt &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay does not affect untracked files
# ---------------------------------------------------------------------------
test_expect_failure 'checkout --no-overlay does not remove untracked files' '
	cd repo &&
	echo "untracked" >untracked.txt &&
	git checkout --no-overlay other -- . &&
	test -f untracked.txt &&
	rm -f untracked.txt &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay with nested directory removal
# ---------------------------------------------------------------------------
test_expect_success 'setup branch with deep nesting' '
	cd repo &&
	git checkout -b deep-branch &&
	mkdir -p a/b/c &&
	echo "deep" >a/b/c/file.txt &&
	git add . &&
	git commit -m "deep nesting" &&
	git checkout master
'

test_expect_failure 'checkout --no-overlay deep-branch -- . adds deep files and removes master-only' '
	cd repo &&
	git checkout --no-overlay deep-branch -- . &&
	test -f a/b/c/file.txt &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay vs --overlay (explicit overlay)
# ---------------------------------------------------------------------------
test_expect_failure 'checkout --overlay other -- . preserves files (contrast)' '
	cd repo &&
	git checkout --overlay other -- . 2>/dev/null &&
	test -f master-only.txt &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay with commit SHA
# ---------------------------------------------------------------------------
test_expect_failure 'checkout --no-overlay <sha> -- . works' '
	cd repo &&
	other_sha=$(git rev-parse other) &&
	git checkout --no-overlay "$other_sha" -- . &&
	test_path_is_missing master-only.txt &&
	test -f other-only.txt &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay with pathspec that matches nothing on source
# ---------------------------------------------------------------------------
test_expect_failure 'checkout --no-overlay with glob pathspec' '
	cd repo &&
	echo "extra-m1" >m1.log &&
	echo "extra-m2" >m2.log &&
	git add m1.log m2.log &&
	git commit -m "add logs" &&
	git checkout --no-overlay other -- "*.txt" &&
	test_path_is_missing master-only.txt &&
	test -f m1.log &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay idempotency
# ---------------------------------------------------------------------------
test_expect_failure 'running --no-overlay twice yields same result' '
	cd repo &&
	git checkout --no-overlay other -- . &&
	git ls-files >first &&
	git checkout --no-overlay other -- . &&
	git ls-files >second &&
	test_cmp first second &&
	git checkout master -- . &&
	git checkout -f master
'

# ---------------------------------------------------------------------------
# --no-overlay does not switch branch
# ---------------------------------------------------------------------------
test_expect_failure 'checkout --no-overlay other -- . does not switch branch' '
	cd repo &&
	git checkout --no-overlay other -- . &&
	test "$(git symbolic-ref --short HEAD)" = "master" &&
	git checkout master -- . &&
	git checkout -f master
'

test_done
