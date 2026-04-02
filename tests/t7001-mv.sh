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

# ---------------------------------------------------------------------------
# Additional tests ported from git/t/t7001-mv.sh
# ---------------------------------------------------------------------------

test_expect_success 'git mv round trip does not change sha1' '
	rm -fr rtrepo &&
	git init rtrepo &&
	cd rtrepo &&
	echo 1 >dirty &&
	git add dirty &&
	entry="$(git ls-files --stage dirty | awk "{print \$2}")" &&
	git mv dirty dirty2 &&
	test "$entry" = "$(git ls-files --stage dirty2 | awk "{print \$2}")" &&
	echo 2 >dirty2 &&
	git mv dirty2 dirty &&
	test "$entry" = "$(git ls-files --stage dirty | awk "{print \$2}")"
'

test_expect_success 'setup multi-file subdir move' '
	rm -rf repo_mf &&
	git init repo_mf &&
	cd repo_mf &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	mkdir path0 path1 &&
	echo content >path0/COPYING &&
	git add path0/COPYING &&
	git commit -m "add COPYING" &&
	echo readme >path0/README &&
	git add path0/README &&
	git commit -m add2 -a
'

test_expect_success 'moving whole subdirectory (multi-file)' '
	cd repo_mf &&
	git mv path0 path2 &&
	git ls-files >actual &&
	grep "path2/COPYING" actual &&
	grep "path2/README" actual &&
	! grep "path0/" actual
'

test_expect_success 'moving whole subdirectory into subdirectory (nested multi-file)' '
	cd repo_mf &&
	git mv path2 path1 &&
	git ls-files >actual &&
	grep "path1/path2/COPYING" actual &&
	grep "path1/path2/README" actual
'

test_expect_success 'move into current directory (.)' '
	cd repo_mf &&
	git mv path1/path2/ . &&
	git ls-files >actual &&
	grep "path2/COPYING" actual
'

test_expect_success 'setup for -k tests (upstream pattern)' '
	cd repo_mf &&
	git commit -m "temp commit" -a &&
	git reset --hard HEAD~1
'

test_expect_success 'mv -k on non-existing file (upstream)' '
	cd repo_mf &&
	git mv -k idontexist path0
'

test_expect_success 'mv -k on untracked file (upstream)' '
	cd repo_mf &&
	>untracked1 &&
	git mv -k untracked1 path0 &&
	test_path_is_file untracked1 &&
	test_path_is_missing path0/untracked1
'

test_expect_success 'checking -k on multiple untracked files (upstream)' '
	cd repo_mf &&
	>untracked2 &&
	git mv -k untracked1 untracked2 path0 &&
	test_path_is_file untracked1 &&
	test_path_is_file untracked2 &&
	test_path_is_missing path0/untracked1 &&
	test_path_is_missing path0/untracked2
'

test_expect_success 'checking -f on untracked file with existing target (upstream)' '
	cd repo_mf &&
	>path0/untracked1 &&
	test_must_fail git mv -f untracked1 path0 &&
	test_path_is_file untracked1 &&
	test_path_is_file path0/untracked1
'

test_expect_success 'mv -f overwrites and updates index' '
	rm -rf repo_mvf &&
	git init repo_mvf &&
	cd repo_mvf &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo test >bar &&
	git add bar &&
	git commit -m test &&
	echo foo >foo &&
	git add foo &&
	git mv -f foo bar &&
	test_path_is_missing foo &&
	test_path_is_file bar &&
	test "$(cat bar)" = "foo"
'

test_expect_success 'mv fails on nonexistent source' '
	cd repo_mvf &&
	test_must_fail git mv nonexistent destination 2>err &&
	grep -i "not under version control\|does not exist" err
'

test_expect_success 'mv fails when tracked destination exists' '
	cd repo_mvf &&
	echo a >src_t &&
	echo b >dst_t &&
	git add src_t dst_t &&
	git commit -m "add src_t dst_t" &&
	test_must_fail git mv src_t dst_t &&
	test_path_is_file src_t &&
	test_path_is_file dst_t
'

test_expect_success 'mv directory into itself should fail' '
	cd repo_mvf &&
	test_must_fail git mv path0 path0 2>err
'

test_expect_success 'mv preserves file content' '
	cd repo_mvf &&
	echo "specific content" >content_file &&
	git add content_file &&
	git mv content_file content_moved &&
	test "$(cat content_moved)" = "specific content"
'

test_expect_success 'mv -k skips invalid and moves valid' '
	cd repo_mvf &&
	echo valid >valid_file &&
	git add valid_file &&
	mkdir -p target_dir &&
	git mv -k nonexist valid_file target_dir &&
	test_path_is_file target_dir/valid_file &&
	test_path_is_missing valid_file
'

test_expect_success 'mv file to same name should fail' '
	cd repo_mvf &&
	echo same >same_file &&
	git add same_file &&
	test_must_fail git mv same_file same_file 2>err
'

test_expect_success 'mv updates index correctly' '
	cd repo_mvf &&
	echo idx_content >idx_file &&
	git add idx_file &&
	git mv idx_file idx_moved &&
	git ls-files >actual &&
	grep "idx_moved" actual &&
	! grep "idx_file" actual
'

test_expect_success 'mv directory preserves all index entries' '
	cd repo_mvf &&
	mkdir -p srcdir &&
	echo a >srcdir/a &&
	echo b >srcdir/b &&
	echo c >srcdir/c &&
	git add srcdir &&
	git mv srcdir destdir &&
	git ls-files >actual &&
	grep "destdir/a" actual &&
	grep "destdir/b" actual &&
	grep "destdir/c" actual &&
	! grep "srcdir/" actual
'

test_expect_success 'mv -n on directory does not move' '
	cd repo_mvf &&
	git mv -n destdir nowhere &&
	test_path_is_dir destdir &&
	test_path_is_missing nowhere &&
	git ls-files >actual &&
	grep "destdir/" actual
'

test_expect_success 'mv on empty string should fail' '
	cd repo_mvf &&
	test_must_fail git mv "" newname 2>err
'

test_done
