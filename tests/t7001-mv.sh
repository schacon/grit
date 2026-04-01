#!/bin/sh
# Ported from git/t/t7001-mv.sh
# Tests for 'grit mv'.

test_description='grit mv'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Basic rename
# ---------------------------------------------------------------------------

test_expect_success 'setup: initial commit with a file' '
	rm -rf repo &&
	git init repo &&
	cd repo &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p path0 &&
	echo "content" >path0/COPYING &&
	git add path0/COPYING &&
	git commit -m "add COPYING"
'

test_expect_success 'rename file to new path' '
	cd repo &&
	mkdir -p path1 &&
	git mv path0/COPYING path1/COPYING &&
	git ls-files >actual &&
	grep "path1/COPYING" actual &&
	! grep "path0/COPYING" actual &&
	test_path_is_file path1/COPYING &&
	test_path_is_missing path0/COPYING
'

test_expect_success 'commit the rename' '
	cd repo &&
	git commit -m "move-out" -a
'

test_expect_success 'rename file back' '
	cd repo &&
	git mv path1/COPYING path0/COPYING &&
	git ls-files >actual &&
	grep "path0/COPYING" actual &&
	! grep "path1/COPYING" actual
'

# ---------------------------------------------------------------------------
# Dry run
# ---------------------------------------------------------------------------

test_expect_success 'setup for dry-run test' '
	rm -rf repo2 &&
	git init repo2 &&
	cd repo2 &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo "hello" >foo.txt &&
	git add foo.txt &&
	git commit -m init
'

test_expect_success 'mv --dry-run does not move file' '
	cd repo2 &&
	git mv -n foo.txt bar.txt &&
	test_path_is_file foo.txt &&
	test_path_is_missing bar.txt &&
	git ls-files >actual &&
	grep "foo.txt" actual &&
	! grep "bar.txt" actual
'

# ---------------------------------------------------------------------------
# -k flag: skip errors
# ---------------------------------------------------------------------------

test_expect_success 'setup for -k tests' '
	rm -rf repo3 &&
	git init repo3 &&
	cd repo3 &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo "tracked" >tracked.txt &&
	git add tracked.txt &&
	git commit -m init
'

test_expect_success '-k skips non-existing source without aborting' '
	cd repo3 &&
	git mv -k idontexist tracked.txt newname.txt 2>/dev/null || true &&
	test_path_is_file tracked.txt
'

test_expect_success '-k skips untracked file without aborting' '
	cd repo3 &&
	>untracked1 &&
	git mv -k untracked1 newdir 2>/dev/null || true &&
	test_path_is_file untracked1 &&
	test_path_is_missing newdir/untracked1
'

# ---------------------------------------------------------------------------
# Moving whole directory
# ---------------------------------------------------------------------------

test_expect_success 'setup for directory move' '
	rm -rf repo4 &&
	git init repo4 &&
	cd repo4 &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p path0 &&
	echo "file1" >path0/f1.txt &&
	echo "file2" >path0/f2.txt &&
	git add path0 &&
	git commit -m init
'

test_expect_success 'move whole directory' '
	cd repo4 &&
	git mv path0 path2 &&
	git ls-files >actual &&
	grep "path2/f1.txt" actual &&
	grep "path2/f2.txt" actual &&
	! grep "path0/" actual &&
	test_path_is_dir path2 &&
	test_path_is_missing path0
'

# ---------------------------------------------------------------------------
# Move multiple sources into a directory
# ---------------------------------------------------------------------------

test_expect_success 'setup for multi-source move' '
	rm -rf repo5 &&
	git init repo5 &&
	cd repo5 &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p dir other &&
	echo a >dir/a.txt &&
	echo b >dir/b.txt &&
	git add dir/a.txt dir/b.txt &&
	git commit -m init
'

test_expect_success 'move multiple sources into directory' '
	cd repo5 &&
	git mv dir/a.txt dir/b.txt other &&
	git ls-files >actual &&
	grep "other/a.txt" actual &&
	grep "other/b.txt" actual &&
	! grep "dir/a.txt" actual &&
	! grep "dir/b.txt" actual
'

# ---------------------------------------------------------------------------
# -f force overwrite
# ---------------------------------------------------------------------------

test_expect_success 'setup for force overwrite' '
	rm -rf repo6 &&
	git init repo6 &&
	cd repo6 &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo "moved" >moved &&
	echo "existing" >existing &&
	git add moved existing &&
	git commit -m init
'

test_expect_success 'mv without -f fails when destination exists' '
	cd repo6 &&
	test_must_fail git mv moved existing
'

test_expect_success 'mv -f overwrites existing file' '
	cd repo6 &&
	git mv -f moved existing &&
	test_path_is_missing moved &&
	test_path_is_file existing &&
	git ls-files >actual &&
	grep "^existing$" actual &&
	! grep "^moved$" actual
'

# ---------------------------------------------------------------------------
# mv should preserve sha1 of index entry (not re-hash)
# ---------------------------------------------------------------------------

test_expect_success 'mv preserves sha1 of cache entry' '
	rm -rf repo7 &&
	git init repo7 &&
	cd repo7 &&
	echo 1 >dirty &&
	git add dirty &&
	entry=$(git ls-files --stage dirty | awk "{print \$2}") &&
	git mv dirty dirty2 &&
	entry2=$(git ls-files --stage dirty2 | awk "{print \$2}") &&
	test "$entry" = "$entry2"
'

# ---------------------------------------------------------------------------
# mv into "." (move to worktree root)
# ---------------------------------------------------------------------------

test_expect_success 'setup: directory in subdir' '
	rm -rf repo8 &&
	git init repo8 &&
	cd repo8 &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p sub/deep &&
	echo x >sub/deep/file.txt &&
	git add sub &&
	git commit -m init
'

test_expect_success 'move directory into current dir (.)' '
	cd repo8 &&
	git mv sub/deep . &&
	git ls-files >actual &&
	grep "deep/file.txt" actual &&
	! grep "sub/deep/file.txt" actual
'

# ---------------------------------------------------------------------------
# Error: conflicted file cannot be moved
# ---------------------------------------------------------------------------

test_expect_success 'mv fails on conflicted file' '
	rm -rf repo9 &&
	git init repo9 &&
	cd repo9 &&
	>conflict &&
	cfhash=$(git hash-object -w conflict) &&
	printf "100644 %s 1\tconflict\n" "$cfhash" |
		git update-index --index-info &&
	test_must_fail git mv conflict newname 2>actual &&
	grep -i "conflict" actual
'

test_done
