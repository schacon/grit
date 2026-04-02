#!/bin/sh
#
# Tests for 'grit checkout' from subdirectories — paths relative to cwd.

test_description='grit checkout from subdirectories'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup
# ---------------------------------------------------------------------------
test_expect_success 'setup repository with nested directories' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	echo "root-file" >root.txt &&
	mkdir -p sub/deep/deeper &&
	echo "sub-file" >sub/file.txt &&
	echo "deep-file" >sub/deep/file.txt &&
	echo "deeper-file" >sub/deep/deeper/file.txt &&
	git add . &&
	git commit -m "initial structure" &&

	git checkout -b other &&
	echo "other-root" >root.txt &&
	echo "other-sub" >sub/file.txt &&
	echo "other-deep" >sub/deep/file.txt &&
	echo "other-deeper" >sub/deep/deeper/file.txt &&
	echo "new-on-other" >sub/other-only.txt &&
	git add . &&
	git commit -m "changes on other branch" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# checkout -- <file> from subdirectory using relative path
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- file from sub/ using relative path' '
	cd repo/sub &&
	echo "dirty" >file.txt &&
	git checkout -- file.txt &&
	test "$(cat file.txt)" = "sub-file"
'

test_expect_success 'checkout -- file from sub/deep/ using relative path' '
	cd repo/sub/deep &&
	echo "dirty" >file.txt &&
	git checkout -- file.txt &&
	test "$(cat file.txt)" = "deep-file"
'

test_expect_success 'checkout -- file from sub/deep/deeper/' '
	cd repo/sub/deep/deeper &&
	echo "dirty" >file.txt &&
	git checkout -- file.txt &&
	test "$(cat file.txt)" = "deeper-file"
'

# ---------------------------------------------------------------------------
# checkout -- parent-relative path (../)
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- ../file.txt from sub/deep/' '
	cd repo/sub/deep &&
	echo "dirty" >../file.txt &&
	git checkout -- ../file.txt &&
	test "$(cat ../file.txt)" = "sub-file"
'

test_expect_success 'checkout -- ../../root.txt from sub/deep/' '
	cd repo/sub/deep &&
	echo "dirty" >../../root.txt &&
	git checkout -- ../../root.txt &&
	test "$(cat ../../root.txt)" = "root-file"
'

# ---------------------------------------------------------------------------
# checkout <branch> -- <file> from subdirectory
# ---------------------------------------------------------------------------
test_expect_success 'checkout other -- file.txt from sub/' '
	cd repo/sub &&
	git checkout other -- file.txt &&
	test "$(cat file.txt)" = "other-sub" &&
	git checkout master -- file.txt
'

test_expect_success 'checkout other -- file.txt from sub/deep/' '
	cd repo/sub/deep &&
	git checkout other -- file.txt &&
	test "$(cat file.txt)" = "other-deep" &&
	git checkout master -- file.txt
'

test_expect_success 'checkout other -- ../file.txt from sub/deep/' '
	cd repo/sub/deep &&
	git checkout other -- ../file.txt &&
	test "$(cat ../file.txt)" = "other-sub" &&
	git checkout master -- ../file.txt
'

# ---------------------------------------------------------------------------
# checkout -- . from subdirectory (restore all in subdir)
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- . from sub/ restores only sub/ files' '
	cd repo &&
	echo "dirty-root" >root.txt &&
	echo "dirty-sub" >sub/file.txt &&
	echo "dirty-deep" >sub/deep/file.txt &&
	cd sub &&
	git checkout -- . &&
	test "$(cat file.txt)" = "sub-file" &&
	test "$(cat deep/file.txt)" = "deep-file" &&
	cd .. &&
	test "$(cat root.txt)" = "dirty-root" &&
	git checkout -- root.txt
'

# ---------------------------------------------------------------------------
# checkout from subdir does not change branch
# ---------------------------------------------------------------------------
test_expect_success 'checkout <commit> -- file from subdir does not switch branch' '
	cd repo/sub &&
	git checkout other -- file.txt &&
	test "$(git symbolic-ref --short HEAD)" = "master" &&
	git checkout master -- file.txt
'

# ---------------------------------------------------------------------------
# checkout branch from subdirectory (branch switching)
# ---------------------------------------------------------------------------
test_expect_success 'checkout branch from subdirectory works' '
	cd repo/sub &&
	git checkout other &&
	test "$(git symbolic-ref --short HEAD)" = "other" &&
	git checkout master
'

test_expect_success 'checkout branch from deep subdirectory' '
	cd repo/sub/deep/deeper &&
	git checkout other &&
	test "$(git symbolic-ref --short HEAD)" = "other" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# checkout -b from subdirectory
# ---------------------------------------------------------------------------
test_expect_success 'checkout -b from subdirectory creates branch' '
	cd repo/sub &&
	git checkout -b sub-created-branch &&
	test "$(git symbolic-ref --short HEAD)" = "sub-created-branch" &&
	git checkout master
'

# ---------------------------------------------------------------------------
# checkout -- nonexistent file from subdir fails
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- nonexistent file from subdir fails' '
	cd repo/sub &&
	test_must_fail git checkout -- no-such-file 2>err &&
	test -s err
'

# ---------------------------------------------------------------------------
# checkout -- directory path from subdir
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- deep/ from sub/ restores deep/ contents' '
	cd repo/sub &&
	echo "dirty-deep" >deep/file.txt &&
	echo "dirty-deeper" >deep/deeper/file.txt &&
	git checkout -- deep &&
	test "$(cat deep/file.txt)" = "deep-file" &&
	test "$(cat deep/deeper/file.txt)" = "deeper-file"
'

# ---------------------------------------------------------------------------
# status from subdirectory (verify grit handles subdir cwd)
# ---------------------------------------------------------------------------
test_expect_success 'git status works from subdirectory' '
	cd repo/sub/deep &&
	git status >../../status-out 2>&1 &&
	test $? -eq 0
'

# ---------------------------------------------------------------------------
# checkout with absolute-from-root pathspec from subdir
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- :/ from subdir restores everything' '
	cd repo &&
	echo "dirty-root" >root.txt &&
	echo "dirty-sub" >sub/file.txt &&
	cd sub &&
	git checkout -- :/ &&
	test "$(cat ../root.txt)" = "root-file" &&
	test "$(cat file.txt)" = "sub-file"
'

# ---------------------------------------------------------------------------
# checkout <commit> -- <path> from subdir with relative path
# ---------------------------------------------------------------------------
test_expect_success 'checkout HEAD -- . from sub/ restores sub tree only' '
	cd repo &&
	echo "dirty-root" >root.txt &&
	echo "dirty-sub" >sub/file.txt &&
	cd sub &&
	git checkout HEAD -- . &&
	test "$(cat file.txt)" = "sub-file" &&
	cd .. &&
	test "$(cat root.txt)" = "dirty-root" &&
	git checkout -- root.txt
'

# ---------------------------------------------------------------------------
# Switching branches from subdir preserves cwd if possible
# ---------------------------------------------------------------------------
test_expect_success 'switching branches from subdir that exists on both' '
	cd repo/sub &&
	git checkout other &&
	test "$(cat file.txt)" = "other-sub" &&
	git checkout master &&
	test "$(cat file.txt)" = "sub-file"
'

# ---------------------------------------------------------------------------
# Multiple relative paths from subdir
# ---------------------------------------------------------------------------
test_expect_success 'checkout -- multiple relative paths from subdir' '
	cd repo/sub &&
	echo "d1" >file.txt &&
	echo "d2" >deep/file.txt &&
	git checkout -- file.txt deep/file.txt &&
	test "$(cat file.txt)" = "sub-file" &&
	test "$(cat deep/file.txt)" = "deep-file"
'

# ---------------------------------------------------------------------------
# checkout -f from subdirectory
# ---------------------------------------------------------------------------
test_expect_success 'checkout -f branch from subdirectory' '
	cd repo/sub &&
	echo "dirty" >file.txt &&
	git checkout -f other &&
	test "$(cat file.txt)" = "other-sub" &&
	git checkout -f master
'

test_done
