#!/bin/sh
#
# Tests that 'reset --hard' removes empty subdirectories.
# Adapted from git/t/t7101-reset-empty-subdirs.sh

test_description='git reset should cull empty subdirs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup: create a repo with nested directories
# ---------------------------------------------------------------------------
test_expect_success 'creating initial files' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	mkdir path0 &&
	echo "initial content" >path0/COPYING &&
	git add path0/COPYING &&
	git commit -m "add path0/COPYING"
'

test_expect_success 'creating second files in nested dirs' '
	cd repo &&
	mkdir -p path1/path2 &&
	echo "nested content" >path1/path2/COPYING &&
	echo "mid content" >path1/COPYING &&
	echo "root content" >COPYING &&
	echo "extra content" >path0/COPYING-TOO &&
	git add path1/path2/COPYING path1/COPYING COPYING path0/COPYING-TOO &&
	git commit -m "add more files"
'

# ---------------------------------------------------------------------------
# Reset --hard HEAD^ removes files AND empty directories
# ---------------------------------------------------------------------------
test_expect_success 'resetting tree HEAD^' '
	cd repo &&
	git reset --hard HEAD^
'

test_expect_success 'checking initial files exist after rewind' '
	cd repo &&
	test_path_is_dir path0 &&
	test_path_is_file path0/COPYING
'

test_expect_success 'checking lack of path1/path2/COPYING' '
	cd repo &&
	test_path_is_missing path1/path2/COPYING
'

test_expect_success 'checking lack of path1/COPYING' '
	cd repo &&
	test_path_is_missing path1/COPYING
'

test_expect_success 'checking lack of COPYING' '
	cd repo &&
	test_path_is_missing COPYING
'

test_expect_success 'checking lack of path0/COPYING-TOO' '
	cd repo &&
	test_path_is_missing path0/COPYING-TOO
'

test_expect_success 'checking lack of path1/path2 (empty subdir removed)' '
	cd repo &&
	test_path_is_missing path1/path2
'

test_expect_success 'checking lack of path1 (empty parent dir removed)' '
	cd repo &&
	test_path_is_missing path1
'

# ---------------------------------------------------------------------------
# Deeper nesting: verify multi-level empty directory removal
# ---------------------------------------------------------------------------
test_expect_success 'setup deeper nesting' '
	cd repo &&
	git reset --hard HEAD &&
	mkdir -p a/b/c/d &&
	echo "deep" >a/b/c/d/file &&
	git add a/b/c/d/file &&
	git commit -m "add deeply nested file"
'

test_expect_success 'reset --hard removes deeply nested empty dirs' '
	cd repo &&
	git reset --hard HEAD^ &&
	test_path_is_missing a/b/c/d/file &&
	test_path_is_missing a/b/c/d &&
	test_path_is_missing a/b/c &&
	test_path_is_missing a/b &&
	test_path_is_missing a
'

# ---------------------------------------------------------------------------
# Partial directory removal: some files remain
# ---------------------------------------------------------------------------
test_expect_success 'setup for partial removal' '
	cd repo &&
	git reset --hard HEAD &&
	mkdir -p sub/keep sub/remove &&
	echo "keep" >sub/keep/file &&
	echo "remove" >sub/remove/file &&
	git add sub &&
	git commit -m "add sub dirs" &&

	git rm sub/remove/file &&
	git commit -m "remove sub/remove/file"
'

test_expect_success 'reset --hard to commit with both dirs restores removed' '
	cd repo &&
	git reset --hard HEAD^ &&
	test -f sub/keep/file &&
	test -f sub/remove/file
'

test_expect_success 'reset --hard forward removes empty dir again' '
	cd repo &&
	# We are at the commit with both sub/keep and sub/remove
	test -f sub/remove/file &&
	# Delete sub/remove/file and commit
	git rm sub/remove/file &&
	git commit -m "remove again" &&
	test_path_is_missing sub/remove &&
	test -f sub/keep/file
'

test_done
