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

# ---------------------------------------------------------------------------
# Additional mv tests
# ---------------------------------------------------------------------------

test_expect_success 'setup: fresh repo for additional mv tests' '
	rm -rf repo_mv2 &&
	git init repo_mv2 &&
	cd repo_mv2 &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo content >file1 &&
	git add file1 &&
	git commit -m "initial"
'

test_expect_success 'mv to directory moves file into it' '
	cd repo_mv2 &&
	mkdir dest &&
	git mv file1 dest/ &&
	test_path_is_file dest/file1 &&
	test_path_is_missing file1 &&
	git ls-files >actual &&
	grep "dest/file1" actual &&
	! grep -x "file1" actual
'

test_expect_success 'mv commit and continue' '
	cd repo_mv2 &&
	git commit -m "moved to dest" &&
	git mv dest/file1 file1 &&
	git commit -m "moved back"
'

test_expect_success 'mv -f overwrites existing tracked file' '
	cd repo_mv2 &&
	echo src >src_file &&
	echo dst >dst_file &&
	git add src_file dst_file &&
	git commit -m "add src and dst" &&
	git mv -f src_file dst_file &&
	test_path_is_missing src_file &&
	test "$(cat dst_file)" = "src" &&
	git ls-files >actual &&
	grep "dst_file" actual &&
	! grep "src_file" actual
'

test_expect_success 'mv -n does not move file' '
	cd repo_mv2 &&
	git commit -m "after overwrite" -a &&
	echo x >drymv &&
	git add drymv &&
	git commit -m "add drymv" &&
	git mv -n drymv drymv_dest &&
	test_path_is_file drymv &&
	test_path_is_missing drymv_dest &&
	git ls-files --error-unmatch drymv
'

test_expect_success 'mv directory to new name' '
	cd repo_mv2 &&
	mkdir mvdir &&
	echo a >mvdir/a &&
	echo b >mvdir/b &&
	git add mvdir &&
	git commit -m "add mvdir" &&
	git mv mvdir newdir &&
	test_path_is_file newdir/a &&
	test_path_is_file newdir/b &&
	test_path_is_missing mvdir &&
	git ls-files >actual &&
	grep "newdir/a" actual &&
	grep "newdir/b" actual &&
	! grep "mvdir/" actual
'

test_expect_success 'mv multiple files to directory' '
	cd repo_mv2 &&
	git commit -m "moved dir" -a &&
	echo x >ma &&
	echo y >mb &&
	mkdir multidir &&
	git add ma mb &&
	git commit -m "add multi" &&
	git mv ma mb multidir/ &&
	test_path_is_file multidir/ma &&
	test_path_is_file multidir/mb &&
	test_path_is_missing ma &&
	test_path_is_missing mb &&
	git ls-files >actual &&
	grep "multidir/ma" actual &&
	grep "multidir/mb" actual
'

test_expect_success 'mv file with spaces in name' '
	cd repo_mv2 &&
	git commit -m "multi done" -a &&
	echo x >"space file" &&
	git add "space file" &&
	git commit -m "add space file" &&
	git mv "space file" "new space" &&
	test_path_is_missing "space file" &&
	test_path_is_file "new space" &&
	git ls-files >actual &&
	grep "new space" actual
'

test_expect_success 'mv preserves file content' '
	cd repo_mv2 &&
	git commit -m "space done" -a &&
	echo "hello world" >content_file &&
	git add content_file &&
	git commit -m "add content file" &&
	git mv content_file renamed_content &&
	test "$(cat renamed_content)" = "hello world"
'

test_expect_success 'mv updates index correctly' '
	cd repo_mv2 &&
	git mv renamed_content idx_check &&
	git ls-files >actual &&
	grep "idx_check" actual &&
	! grep "renamed_content" actual
'

test_expect_success 'mv then status shows rename' '
	cd repo_mv2 &&
	git commit -m "idx done" -a &&
	echo x >status_mv &&
	git add status_mv &&
	git commit -m "add status_mv" &&
	git mv status_mv status_mv_new &&
	git status --porcelain >actual &&
	grep "status_mv" actual
'

test_expect_success 'setup fresh repo for mv commit test' '
	rm -rf repo_mv_commit &&
	git init repo_mv_commit &&
	cd repo_mv_commit &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo x >commit_mv &&
	git add commit_mv &&
	git commit -m "add commit_mv"
'

test_expect_success 'mv then commit succeeds' '
	cd repo_mv_commit &&
	git mv commit_mv commit_mv_done &&
	git commit -m "rename commit_mv" &&
	git log --oneline -n 1 >actual &&
	grep "rename" actual
'

test_expect_success 'mv directory with multiple files updates all index entries' '
	cd repo_mv2 &&
	mkdir bigdir &&
	echo a >bigdir/a &&
	echo b >bigdir/b &&
	echo c >bigdir/c &&
	git add bigdir &&
	git commit -m "add bigdir" &&
	git mv bigdir renameddir &&
	git ls-files >actual &&
	grep "renameddir/a" actual &&
	grep "renameddir/b" actual &&
	grep "renameddir/c" actual &&
	! grep "bigdir/" actual
'

test_expect_success 'mv file into newly created directory' '
	cd repo_mv2 &&
	git commit -m "bigdir done" -a &&
	echo x >newdir_mv &&
	git add newdir_mv &&
	git commit -m "add newdir_mv" &&
	mkdir fresh_dir &&
	git mv newdir_mv fresh_dir/ &&
	test_path_is_file fresh_dir/newdir_mv &&
	test_path_is_missing newdir_mv
'

test_expect_success 'mv -n multiple files to directory' '
	cd repo_mv2 &&
	git commit -m "freshdir done" -a &&
	echo a >dn1 &&
	echo b >dn2 &&
	mkdir dndir &&
	git add dn1 dn2 &&
	git commit -m "add dn files" &&
	git mv -n dn1 dn2 dndir/ &&
	test_path_is_file dn1 &&
	test_path_is_file dn2 &&
	test_path_is_missing dndir/dn1 &&
	test_path_is_missing dndir/dn2
'

test_expect_success 'mv preserves executable bit' '
	cd repo_mv2 &&
	echo "#!/bin/sh" >exec_file &&
	chmod +x exec_file &&
	git add exec_file &&
	git commit -m "add exec_file" &&
	git mv exec_file exec_renamed &&
	test -x exec_renamed
'

test_expect_success 'mv then diff --cached shows rename' '
	cd repo_mv2 &&
	git commit -m "exec done" -a &&
	echo x >diff_mv &&
	git add diff_mv &&
	git commit -m "add diff_mv" &&
	git mv diff_mv diff_mv_new &&
	git diff --cached --name-status >actual &&
	grep "diff_mv" actual
'

test_expect_success 'mv directory with subdirectories' '
	cd repo_mv2 &&
	git commit -m "diff done" -a &&
	mkdir -p deep/sub1/sub2 &&
	echo a >deep/f &&
	echo b >deep/sub1/g &&
	echo c >deep/sub1/sub2/h &&
	git add deep &&
	git commit -m "add deep" &&
	git mv deep deep_new &&
	git ls-files >actual &&
	grep "deep_new/f" actual &&
	grep "deep_new/sub1/g" actual &&
	grep "deep_new/sub1/sub2/h" actual &&
	! grep -E "^deep/" actual
'

test_expect_success 'mv -f over tracked file updates index correctly' '
	cd repo_mv2 &&
	git commit -m "deep done" -a &&
	echo src_content >fsrc &&
	echo dst_content >fdst &&
	git add fsrc fdst &&
	git commit -m "add fsrc fdst" &&
	git mv -f fsrc fdst &&
	git ls-files >actual &&
	grep "fdst" actual &&
	! grep "fsrc" actual &&
	git diff --cached --name-status >status &&
	grep "fsrc" status
'

test_expect_success 'mv dir into another dir' '
	cd repo_mv2 &&
	git commit -m "force done" -a &&
	mkdir d_into1 &&
	echo x >d_into1/f &&
	mkdir d_into2 &&
	git add d_into1 &&
	git commit -m "add d_into1" &&
	git mv d_into1 d_into2/ &&
	test_path_is_file d_into2/d_into1/f &&
	test_path_is_missing d_into1 &&
	git ls-files >actual &&
	grep "d_into2/d_into1/f" actual
'

test_expect_success 'mv dir to nested destination' '
	cd repo_mv2 &&
	git commit -m "into done" -a &&
	mkdir nest_src &&
	echo x >nest_src/f &&
	git add nest_src &&
	git commit -m "add nest_src" &&
	mkdir -p nest_dest &&
	git mv nest_src nest_dest/nest_src &&
	test_path_is_file nest_dest/nest_src/f &&
	test_path_is_missing nest_src
'

test_expect_success 'mv file to same dir different name' '
	cd repo_mv2 &&
	git commit -m "nest done" -a &&
	echo x >hello_mv &&
	git add hello_mv &&
	git commit -m "add hello_mv" &&
	git mv hello_mv world_mv &&
	test_path_is_missing hello_mv &&
	test_path_is_file world_mv &&
	git ls-files >actual &&
	grep "world_mv" actual &&
	! grep "hello_mv" actual
'

test_expect_success 'mv file into nested new name' '
	cd repo_mv2 &&
	git commit -m "world done" -a &&
	echo x >nest_name &&
	git add nest_name &&
	git commit -m "add nest_name" &&
	mkdir nsub &&
	git mv nest_name nsub/new_name &&
	test_path_is_file nsub/new_name &&
	test_path_is_missing nest_name
'

test_expect_success 'mv -f src dst where dst is untracked' '
	cd repo_mv2 &&
	git commit -m "nsub done" -a &&
	echo x >tracked_f &&
	git add tracked_f &&
	git commit -m "add tracked_f" &&
	echo y >untracked_g &&
	git mv -f tracked_f untracked_g &&
	test_path_is_file untracked_g &&
	test_path_is_missing tracked_f &&
	git ls-files >actual &&
	grep "untracked_g" actual
'

test_expect_success 'mv -n on directory does not change anything' '
	cd repo_mv2 &&
	git commit -m "untracked done" -a &&
	mkdir dry_dir &&
	echo a >dry_dir/a &&
	echo b >dry_dir/b &&
	git add dry_dir &&
	git commit -m "add dry_dir" &&
	git mv -n dry_dir nowhere &&
	test_path_is_dir dry_dir &&
	test_path_is_missing nowhere &&
	git ls-files >actual &&
	grep "dry_dir/" actual
'

test_expect_success 'mv overwrites destination content' '
	cd repo_mv2 &&
	echo "overwrite_src" >ow_src &&
	echo "overwrite_dst" >ow_dst &&
	git add ow_src ow_dst &&
	git commit -m "add ow files" &&
	git mv -f ow_src ow_dst &&
	test "$(cat ow_dst)" = "overwrite_src"
'

test_expect_success 'mv directory keeps all file contents' '
	cd repo_mv2 &&
	git commit -m "ow done" -a &&
	mkdir keep_dir &&
	echo alpha >keep_dir/a &&
	echo bravo >keep_dir/b &&
	git add keep_dir &&
	git commit -m "add keep_dir" &&
	git mv keep_dir kept_dir &&
	test "$(cat kept_dir/a)" = "alpha" &&
	test "$(cat kept_dir/b)" = "bravo"
'

test_expect_success 'mv then re-mv back to original' '
	cd repo_mv2 &&
	git commit -m "keep done" -a &&
	echo x >roundtrip &&
	git add roundtrip &&
	git commit -m "add roundtrip" &&
	git mv roundtrip temp_name &&
	git mv temp_name roundtrip &&
	test_path_is_file roundtrip &&
	git ls-files >actual &&
	grep "roundtrip" actual
'

test_expect_success 'mv two files swapping requires intermediate' '
	cd repo_mv2 &&
	git commit --allow-empty -m "roundtrip done" &&
	echo aa >swap_a &&
	echo bb >swap_b &&
	git add swap_a swap_b &&
	git commit -m "add swap files" &&
	git mv swap_a swap_tmp &&
	git mv swap_b swap_a &&
	git mv swap_tmp swap_b &&
	test "$(cat swap_a)" = "bb" &&
	test "$(cat swap_b)" = "aa" &&
	git ls-files >actual &&
	grep "swap_a" actual &&
	grep "swap_b" actual
'

# === additional deepening tests ===

test_expect_success 'mv file to new subdirectory' '
	cd repo_mv2 &&
	git commit --allow-empty -m "swap done" &&
	echo mvd >mv_to_dir.txt &&
	git add mv_to_dir.txt && git commit -m "add mv_to_dir" &&
	mkdir -p mv_target_dir &&
	grit mv mv_to_dir.txt mv_target_dir/ &&
	test_path_is_file mv_target_dir/mv_to_dir.txt &&
	! test -f mv_to_dir.txt &&
	git ls-files >actual &&
	grep "mv_target_dir/mv_to_dir.txt" actual
'

test_expect_success 'mv preserves file content' '
	cd repo_mv2 &&
	git commit -m "mv_to_dir done" -a 2>/dev/null &&
	echo preserve_content >preserve.txt &&
	git add preserve.txt && git commit -m "add preserve" &&
	grit mv preserve.txt preserved.txt &&
	test "$(cat preserved.txt)" = "preserve_content"
'

test_expect_success 'mv nonexistent source fails' '
	cd repo_mv2 &&
	git commit -m "preserve done" -a 2>/dev/null &&
	test_must_fail grit mv no_such_file.txt dest.txt 2>/dev/null
'

test_expect_success 'mv to existing file fails without -f' '
	cd repo_mv2 &&
	echo src >mv_clash_src.txt && echo dst >mv_clash_dst.txt &&
	git add mv_clash_src.txt mv_clash_dst.txt && git commit -m "add clash" &&
	test_must_fail grit mv mv_clash_src.txt mv_clash_dst.txt 2>/dev/null
'

test_expect_success 'mv -f overwrites existing destination' '
	cd repo_mv2 &&
	grit mv -f mv_clash_src.txt mv_clash_dst.txt &&
	test "$(cat mv_clash_dst.txt)" = "src" &&
	! test -f mv_clash_src.txt
'

test_expect_success 'mv updates index with new path' '
	cd repo_mv2 &&
	git commit -m "clash done" -a 2>/dev/null &&
	echo idx >mv_idx.txt &&
	git add mv_idx.txt && git commit -m "add idx" &&
	grit mv mv_idx.txt mv_idx_new.txt &&
	git ls-files >../actual &&
	grep "mv_idx_new.txt" ../actual &&
	! grep "^mv_idx.txt$" ../actual
'

test_expect_success 'mv file with spaces in name' '
	cd repo_mv2 &&
	git commit -m "idx done" -a 2>/dev/null &&
	echo sp >"mv space src.txt" &&
	git add "mv space src.txt" && git commit -m "add space" &&
	grit mv "mv space src.txt" "mv space dst.txt" &&
	test_path_is_file "mv space dst.txt" &&
	! test -f "mv space src.txt"
'

test_expect_success 'mv -k skips errors and continues' '
	cd repo_mv2 &&
	git commit -m "space done" -a 2>/dev/null &&
	echo kk >mv_k_file.txt &&
	git add mv_k_file.txt && git commit -m "add k" &&
	mkdir -p mv_k_dir &&
	grit mv -k nonexist.txt mv_k_file.txt mv_k_dir/ 2>/dev/null &&
	test_path_is_file mv_k_dir/mv_k_file.txt
'

test_expect_success 'mv directory moves all contents' '
	cd repo_mv2 &&
	git commit -m "k done" -a 2>/dev/null &&
	mkdir -p mv_dir_src &&
	echo a >mv_dir_src/a.txt && echo b >mv_dir_src/b.txt &&
	git add mv_dir_src && git commit -m "add dir_src" &&
	grit mv mv_dir_src mv_dir_dst &&
	test_path_is_file mv_dir_dst/a.txt &&
	test_path_is_file mv_dir_dst/b.txt &&
	! test -d mv_dir_src
'

test_expect_success 'mv file up from subdirectory' '
	cd repo_mv2 &&
	git commit -m "dir done" -a 2>/dev/null &&
	mkdir -p mv_up_sub &&
	echo up >mv_up_sub/up.txt &&
	git add mv_up_sub && git commit -m "add up" &&
	grit mv mv_up_sub/up.txt mv_up_here.txt &&
	test_path_is_file mv_up_here.txt &&
	! test -f mv_up_sub/up.txt
'

test_expect_success 'mv --dry-run does not move file' '
	cd repo_mv2 &&
	git commit -m "up done" -a 2>/dev/null &&
	echo dry >mv_dry.txt &&
	git add mv_dry.txt && git commit -m "add dry" &&
	grit mv --dry-run mv_dry.txt mv_dry_dst.txt 2>/dev/null &&
	test_path_is_file mv_dry.txt &&
	! test -f mv_dry_dst.txt
'

test_expect_success 'mv multiple files to directory' '
	cd repo_mv2 &&
	echo m1 >mv_multi1.txt && echo m2 >mv_multi2.txt &&
	git add mv_multi1.txt mv_multi2.txt && git commit -m "add multi" &&
	mkdir -p mv_multi_dst &&
	grit mv mv_multi1.txt mv_multi2.txt mv_multi_dst/ &&
	test_path_is_file mv_multi_dst/mv_multi1.txt &&
	test_path_is_file mv_multi_dst/mv_multi2.txt
'

test_expect_success 'mv shows rename in status after move' '
	cd repo_mv2 &&
	git commit -m "multi done" -a 2>/dev/null &&
	echo stat >mv_stat.txt &&
	git add mv_stat.txt && git commit -m "add stat" &&
	grit mv mv_stat.txt mv_stat_new.txt &&
	git ls-files >../actual &&
	grep "mv_stat_new.txt" ../actual
'

test_expect_success 'mv to same name fails' '
	cd repo_mv2 &&
	git commit -m "stat done" -a 2>/dev/null &&
	echo nd >mv_same.txt &&
	git add mv_same.txt && git commit -m "add same" &&
	test_must_fail grit mv mv_same.txt mv_same.txt 2>/dev/null
'

test_expect_success 'mv then commit records moved file' '
	cd repo_mv2 &&
	echo rec >mv_record.txt &&
	git add mv_record.txt && git commit -m "add record" &&
	grit mv mv_record.txt mv_recorded.txt &&
	git commit -m "moved record" 2>/dev/null &&
	git ls-files >../actual &&
	grep "mv_recorded.txt" ../actual &&
	! grep "^mv_record.txt$" ../actual
'

test_done
