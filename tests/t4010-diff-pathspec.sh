#!/bin/sh
test_description='grit diff pathspec matching'

. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	echo "aaa" >alpha.txt &&
	echo "bbb" >beta.rs &&
	mkdir sub &&
	echo "ccc" >sub/deep.txt &&
	echo "ddd" >sub/extra.rs &&
	git add . &&
	git commit -m "initial" &&
	echo "AAA" >alpha.txt &&
	echo "BBB" >beta.rs &&
	echo "CCC" >sub/deep.txt &&
	echo "DDD" >sub/extra.rs
'

# --- diff HEAD -- <path> (working tree vs HEAD) ---

test_expect_success 'diff HEAD -- single file shows only that file' '
	cd repo &&
	git diff HEAD -- alpha.txt >out &&
	grep "alpha\.txt" out &&
	! grep "beta\.rs" out &&
	! grep "sub/" out
'

test_expect_success 'diff HEAD -- subdirectory restricts to subdir' '
	cd repo &&
	git diff HEAD -- sub/ >out &&
	grep "sub/deep\.txt" out &&
	grep "sub/extra\.rs" out &&
	! grep "alpha\.txt" out
'

test_expect_success 'diff HEAD -- multiple paths' '
	cd repo &&
	git diff HEAD -- alpha.txt beta.rs >out &&
	grep "alpha\.txt" out &&
	grep "beta\.rs" out &&
	! grep "sub/" out
'

test_expect_success 'diff HEAD -- nonexistent path gives empty output' '
	cd repo &&
	git diff HEAD -- nonexistent.txt >out &&
	test_must_be_empty out
'

# --- diff --name-only / --name-status with pathspec ---

test_expect_success 'diff --name-only HEAD -- restricts to path' '
	cd repo &&
	git diff --name-only HEAD -- alpha.txt >out &&
	grep "alpha\.txt" out &&
	! grep "beta" out
'

test_expect_success 'diff --name-status HEAD -- restricts to path' '
	cd repo &&
	git diff --name-status HEAD -- sub/ >out &&
	grep "sub/deep\.txt" out &&
	! grep "alpha" out
'

test_expect_success 'diff --stat HEAD -- restricts to path' '
	cd repo &&
	git diff --stat HEAD -- alpha.txt >out &&
	grep "alpha" out &&
	! grep "beta" out
'

test_expect_success 'diff --numstat HEAD -- restricts to path' '
	cd repo &&
	git diff --numstat HEAD -- alpha.txt >out &&
	grep "alpha" out &&
	! grep "beta" out
'

# --- diff-tree with pathspec ---

test_expect_success 'setup commits for diff-tree pathspec' '
	cd repo &&
	git add . &&
	git commit -m "modified" &&
	printf "%s\n" "$(git rev-parse HEAD~1)" >../c1 &&
	printf "%s\n" "$(git rev-parse HEAD)" >../c2
'

test_expect_success 'diff-tree -- single file restricts output' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	git diff-tree -r "$c1" "$c2" -- alpha.txt >out &&
	grep "alpha\.txt" out &&
	test_line_count = 1 out
'

test_expect_success 'diff-tree -- subdirectory' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	git diff-tree -r "$c1" "$c2" -- sub/ >out &&
	grep "sub/deep\.txt" out &&
	grep "sub/extra\.rs" out &&
	! grep "alpha" out
'

test_expect_success 'diff-tree -- nonexistent path gives empty' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	git diff-tree -r "$c1" "$c2" -- no-such-file >out &&
	test_must_be_empty out
'

test_expect_success 'diff-tree --name-only with pathspec' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	git diff-tree -r --name-only "$c1" "$c2" -- sub/ >out &&
	grep "sub/deep\.txt" out &&
	grep "sub/extra\.rs" out &&
	! grep "alpha" out
'

test_expect_success 'diff-tree --name-status with pathspec' '
	cd repo &&
	c1=$(cat ../c1) && c2=$(cat ../c2) &&
	git diff-tree -r --name-status "$c1" "$c2" -- alpha.txt >out &&
	grep "alpha\.txt" out &&
	test_line_count = 1 out
'

# --- diff --cached with pathspec (needs explicit HEAD) ---

test_expect_success 'setup for cached pathspec' '
	cd repo &&
	echo "new-alpha" >alpha.txt &&
	echo "new-beta" >beta.rs &&
	git add alpha.txt beta.rs
'

test_expect_success 'diff --cached HEAD -- single file' '
	cd repo &&
	git diff --cached HEAD -- alpha.txt >out &&
	grep "alpha\.txt" out &&
	! grep "beta" out
'

test_expect_success 'diff --cached HEAD -- different file' '
	cd repo &&
	git diff --cached HEAD -- beta.rs >out &&
	grep "beta\.rs" out &&
	! grep "alpha" out
'

test_done
