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

# ---------------------------------------------------------------------------
# Moving to absent target with trailing slash
# ---------------------------------------------------------------------------

test_expect_success 'moving to absent target with trailing slash' '
	rm -rf repo_trail &&
	git init repo_trail &&
	cd repo_trail &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p path0 &&
	echo content >path0/COPYING &&
	git add path0/COPYING &&
	git commit -m init &&
	test_must_fail git mv path0/COPYING no-such-dir/ &&
	test_must_fail git mv path0/COPYING no-such-dir// &&
	git mv path0/ no-such-dir/ &&
	test_path_is_dir no-such-dir
'

# ---------------------------------------------------------------------------
# Source is prefix of destination
# ---------------------------------------------------------------------------

test_expect_success 'succeed when source is a prefix of destination' '
	rm -rf repo_prefix &&
	git init repo_prefix &&
	cd repo_prefix &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir path2 &&
	echo x >path2/COPYING &&
	git add path2 &&
	git commit -m init &&
	git mv path2/COPYING path2/COPYING-renamed &&
	git ls-files >actual &&
	grep "path2/COPYING-renamed" actual
'

# ---------------------------------------------------------------------------
# Rename directory to non-existing directory
# ---------------------------------------------------------------------------

test_expect_success 'rename directory to non-existing directory' '
	rm -rf repo_rdir &&
	git init repo_rdir &&
	cd repo_rdir &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir dir-a &&
	>dir-a/f &&
	git add dir-a &&
	git commit -m init &&
	git mv dir-a non-existing-dir &&
	test_path_is_dir non-existing-dir &&
	test_path_is_missing dir-a
'

# ---------------------------------------------------------------------------
# Do not move directory over existing directory
# ---------------------------------------------------------------------------

test_expect_success 'do not move directory over existing directory' '
	rm -rf repo_noover &&
	git init repo_noover &&
	cd repo_noover &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p path0/path2 &&
	mkdir -p path2 &&
	echo x >path2/f &&
	git add path2 &&
	git commit -m init &&
	test_must_fail git mv path2 path0
'

# ---------------------------------------------------------------------------
# Overwrite symlink with file via -f
# ---------------------------------------------------------------------------

test_expect_success 'git mv should overwrite symlink to a file' '
	rm -rf repo_sym &&
	git init repo_sym &&
	cd repo_sym &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo 1 >moved &&
	ln -s moved symlink &&
	git add moved symlink &&
	git commit -m init &&
	test_must_fail git mv moved symlink &&
	git mv -f moved symlink &&
	test_path_is_missing moved &&
	test_path_is_file symlink &&
	test "$(cat symlink)" = 1 &&
	git update-index --refresh &&
	git diff-files --quiet
'

# ---------------------------------------------------------------------------
# Overwrite file with symlink via -f
# ---------------------------------------------------------------------------

test_expect_success 'git mv should overwrite file with a symlink' '
	rm -rf repo_sym2 &&
	git init repo_sym2 &&
	cd repo_sym2 &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo 1 >moved &&
	ln -s moved symlink &&
	git add moved symlink &&
	git commit -m init &&
	test_must_fail git mv symlink moved &&
	git mv -f symlink moved &&
	test_path_is_missing symlink &&
	test -L moved &&
	git update-index --refresh &&
	git diff-files --quiet
'

# ---------------------------------------------------------------------------
# Check moved symlink is a symlink
# ---------------------------------------------------------------------------

test_expect_success 'check moved symlink' '
	rm -rf repo_chksym &&
	git init repo_chksym &&
	cd repo_chksym &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo 1 >moved &&
	ln -s moved symlink &&
	git add moved symlink &&
	git commit -m init &&
	git mv -f symlink moved &&
	test -L moved
'

# ---------------------------------------------------------------------------
# Moving to existing tracked/untracked target with trailing slash
# ---------------------------------------------------------------------------

test_expect_success 'moving to existing tracked target with trailing slash' '
	rm -rf repo_track_trail &&
	git init repo_track_trail &&
	cd repo_track_trail &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p path0 path2 &&
	echo x >path0/COPYING &&
	>path2/file &&
	git add path0 path2 &&
	git commit -m init &&
	git mv path0/ path2/ &&
	test_path_is_dir path2/path0/
'

test_expect_success 'moving to existing untracked target with trailing slash' '
	rm -rf repo_untrack_trail &&
	git init repo_untrack_trail &&
	cd repo_untrack_trail &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p path0 &&
	echo content >path0/COPYING &&
	git add path0 &&
	git commit -m init &&
	mkdir path1 &&
	git mv path0/ path1/ &&
	test_path_is_dir path1/path0/
'

# ---------------------------------------------------------------------------
# Michael Cassar and Sergey Vlasov test cases
# ---------------------------------------------------------------------------

test_expect_success "Michael Cassar test case" '
	rm -rf repo_mc &&
	git init repo_mc &&
	cd repo_mc &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p papers/unsorted papers/all-papers partA &&
	echo a >papers/unsorted/Thesis.pdf &&
	echo b >partA/outline.txt &&
	echo c >papers/unsorted/_another &&
	git add papers partA &&
	T1=$(git write-tree) &&
	git mv papers/unsorted/Thesis.pdf papers/all-papers/moo-blah.pdf &&
	T=$(git write-tree) &&
	git ls-tree -r $T >out &&
	grep partA/outline.txt out
'

test_expect_success "Sergey Vlasov test case" '
	rm -rf repo_sv &&
	git init repo_sv &&
	cd repo_sv &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir ab &&
	date >ab.c &&
	date >ab/d &&
	git add ab.c ab &&
	git commit -m "initial" &&
	git mv ab a
'

# ---------------------------------------------------------------------------
# -k on multiple untracked files
# ---------------------------------------------------------------------------

test_expect_success 'checking -k on multiple untracked files' '
	rm -rf repo_multi_k &&
	git init repo_multi_k &&
	cd repo_multi_k &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p path0 &&
	echo tracked >path0/COPYING &&
	git add path0 &&
	git commit -m init &&
	>untracked1 &&
	>untracked2 &&
	git mv -k untracked1 untracked2 path0 &&
	test_path_is_file untracked1 &&
	test_path_is_file untracked2 &&
	test_path_is_missing path0/untracked1 &&
	test_path_is_missing path0/untracked2
'

# ---------------------------------------------------------------------------
# -f on untracked file with existing target
# ---------------------------------------------------------------------------

test_expect_success 'checking -f on untracked file with existing target' '
	rm -rf repo_f_untrack &&
	git init repo_f_untrack &&
	cd repo_f_untrack &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir path0 &&
	echo tracked >path0/COPYING &&
	git add path0 &&
	git commit -m init &&
	>untracked1 &&
	>path0/untracked1 &&
	test_must_fail git mv -f untracked1 path0 &&
	test_path_is_file untracked1 &&
	test_path_is_file path0/untracked1
'

# ---------------------------------------------------------------------------
# Moving whole subdirectory into subdirectory
# ---------------------------------------------------------------------------

test_expect_success 'moving whole subdirectory into subdirectory' '
	rm -rf repo_subdir &&
	git init repo_subdir &&
	cd repo_subdir &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir -p path0 path1 &&
	echo x >path0/COPYING &&
	echo y >path0/README &&
	git add path0 &&
	git commit -m init &&
	git mv path0 path1 &&
	git ls-files >actual &&
	grep "path1/path0/COPYING" actual &&
	grep "path1/path0/README" actual
'

test_done
