#!/bin/sh
# Tests for 'grit read-tree' with directory/file conflicts.
# Ported from git/t/t1012-read-tree-df.sh
#
# NOTE: grit's read-tree -m -u has issues replacing directories with
# files in the working tree (ENOTDIR). Those tests use test_expect_success.

test_description='grit read-tree directory/file conflicts'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: base tree with file' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo "base" >path &&
	git add path &&
	git commit -m "file at path" &&
	git tag file-at-path &&
	git rev-parse file-at-path^{tree} >../tree_file
'

test_expect_success 'setup: tree with directory where file was' '
	cd repo &&
	git rm path &&
	mkdir path &&
	echo "inside" >path/subfile &&
	git add path/subfile &&
	git commit -m "dir at path" &&
	git tag dir-at-path &&
	git rev-parse dir-at-path^{tree} >../tree_dir
'

test_expect_success 'read-tree switches from file to dir in index' '
	cd repo &&
	T_FILE=$(cat ../tree_file) &&
	T_DIR=$(cat ../tree_dir) &&
	rm -f .git/index &&
	git read-tree "$T_FILE" &&
	git read-tree -m -u "$T_FILE" "$T_DIR" &&
	git ls-files >actual &&
	grep "path/subfile" actual &&
	! grep "^path$" actual
'

test_expect_success 'working tree updated: dir replaces file' '
	cd repo &&
	T_FILE=$(cat ../tree_file) &&
	T_DIR=$(cat ../tree_dir) &&
	rm -f .git/index &&
	git read-tree "$T_FILE" &&
	git checkout-index -f -a &&
	test -f path &&
	git read-tree -m -u "$T_FILE" "$T_DIR" &&
	test -d path &&
	test -f path/subfile
'

test_expect_success 'read-tree switches from dir to file in index' '
	cd repo &&
	T_FILE=$(cat ../tree_file) &&
	T_DIR=$(cat ../tree_dir) &&
	rm -f .git/index &&
	git read-tree "$T_DIR" &&
	git read-tree -m -u "$T_DIR" "$T_FILE" &&
	git ls-files >actual &&
	grep "^path$" actual &&
	! grep "path/subfile" actual
'

test_expect_success 'working tree updated: file replaces dir (ENOTDIR)' '
	cd repo &&
	T_FILE=$(cat ../tree_file) &&
	T_DIR=$(cat ../tree_dir) &&
	rm -rf path &&
	rm -f .git/index &&
	git read-tree "$T_DIR" &&
	git checkout-index -f -a &&
	test -d path &&
	git read-tree -m -u "$T_DIR" "$T_FILE" &&
	test -f path &&
	! test -d path
'

test_expect_success 'setup: deeper nesting via index manipulation' '
	cd repo &&
	rm -rf path &&
	rm -f .git/index &&
	mkdir -p path/deep/nested &&
	echo "deep" >path/deep/nested/file &&
	git add path/deep/nested/file &&
	git commit -m "deeply nested dir at path" &&
	git tag deep-dir &&
	git rev-parse deep-dir^{tree} >../tree_deep
'

test_expect_success 'read-tree from file to deeply nested dir' '
	cd repo &&
	T_FILE=$(cat ../tree_file) &&
	T_DEEP=$(cat ../tree_deep) &&
	rm -rf path &&
	rm -f .git/index &&
	git read-tree "$T_FILE" &&
	git checkout-index -f -a &&
	git read-tree -m -u "$T_FILE" "$T_DEEP" &&
	git ls-files >actual &&
	grep "path/deep/nested/file" actual
'

test_expect_success 'read-tree from deeply nested dir to file (index)' '
	cd repo &&
	T_FILE=$(cat ../tree_file) &&
	T_DEEP=$(cat ../tree_deep) &&
	rm -rf path &&
	rm -f .git/index &&
	git read-tree "$T_DEEP" &&
	git read-tree -m "$T_DEEP" "$T_FILE" &&
	git ls-files >actual &&
	grep "^path$" actual &&
	! grep "path/" actual
'

test_expect_success 'setup: two files, one becomes dir' '
	cd repo &&
	rm -rf path become-dir keep &&
	rm -f .git/index &&
	echo "keep" >keep &&
	echo "change" >become-dir &&
	git add keep become-dir &&
	git commit -m "two files" &&
	git tag two-files &&
	git rev-parse two-files^{tree} >../tree_two &&
	git rm become-dir &&
	mkdir become-dir &&
	echo "sub" >become-dir/sub &&
	git add become-dir/sub &&
	git commit -m "one became dir" &&
	git tag one-dir &&
	git rev-parse one-dir^{tree} >../tree_onedir
'

test_expect_success 'df conflict: unchanged file preserved during switch' '
	cd repo &&
	T_TWO=$(cat ../tree_two) &&
	T_ONEDIR=$(cat ../tree_onedir) &&
	rm -rf become-dir &&
	rm -f .git/index &&
	git read-tree "$T_TWO" &&
	git checkout-index -f -a &&
	git read-tree -m -u "$T_TWO" "$T_ONEDIR" &&
	git ls-files >actual &&
	grep "keep" actual &&
	grep "become-dir/sub" actual &&
	test -f keep
'

test_expect_success 'df conflict: switch back dir->file (ENOTDIR)' '
	cd repo &&
	T_TWO=$(cat ../tree_two) &&
	T_ONEDIR=$(cat ../tree_onedir) &&
	rm -rf become-dir &&
	rm -f .git/index &&
	git read-tree "$T_ONEDIR" &&
	git checkout-index -f -a &&
	git read-tree -m -u "$T_ONEDIR" "$T_TWO" &&
	git ls-files >actual &&
	grep "keep" actual &&
	grep "^become-dir$" actual &&
	! grep "become-dir/" actual
'

test_expect_success '3-way merge with df conflict in index' '
	cd repo &&
	T_TWO=$(cat ../tree_two) &&
	T_ONEDIR=$(cat ../tree_onedir) &&
	rm -rf become-dir &&
	rm -f .git/index &&
	git read-tree -m "$T_TWO" "$T_TWO" "$T_ONEDIR" &&
	git ls-files -s >actual &&
	grep "become-dir" actual
'

test_expect_success '3-way merge: base=file, ours=file, theirs=dir' '
	cd repo &&
	T_TWO=$(cat ../tree_two) &&
	T_ONEDIR=$(cat ../tree_onedir) &&
	rm -rf become-dir &&
	rm -f .git/index &&
	git read-tree -m "$T_TWO" "$T_TWO" "$T_ONEDIR" &&
	git ls-files -s >actual &&
	grep "become-dir" actual &&
	grep "keep" actual
'

test_done
