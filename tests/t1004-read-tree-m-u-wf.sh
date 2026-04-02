#!/bin/sh
# Tests for 'grit read-tree -m -u' with working tree interactions.
# Ported from git/t/t1004-read-tree-m-u-wf.sh

test_description='grit read-tree -m -u with working tree changes'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: create repo with two trees' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	mkdir sub &&
	echo "base" >file1 &&
	echo "sub-base" >sub/file2 &&
	git add file1 sub/file2 &&
	git commit -m "base" &&
	git tag base &&
	git rev-parse base^{tree} >../tree_base &&
	echo "modified" >file1 &&
	echo "new" >file3 &&
	git add file1 file3 &&
	git commit -m "modified" &&
	git tag modified &&
	git rev-parse modified^{tree} >../tree_modified
'

test_expect_success 'read-tree -m -u switches working tree (2-way)' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_MOD=$(cat ../tree_modified) &&
	git checkout base &&
	git read-tree -m -u "$T_BASE" "$T_MOD" &&
	echo "modified" >expect &&
	test_cmp expect file1 &&
	test -f file3
'

test_expect_success 'read-tree -m -u adds new files to working tree' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_MOD=$(cat ../tree_modified) &&
	rm -f .git/index &&
	git read-tree "$T_BASE" &&
	git checkout-index -f -a &&
	git read-tree -m -u "$T_BASE" "$T_MOD" &&
	test -f file3 &&
	echo "new" >expect &&
	test_cmp expect file3
'

test_expect_success 'read-tree -m without -u does not touch working tree' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_MOD=$(cat ../tree_modified) &&
	rm -f .git/index &&
	git read-tree "$T_BASE" &&
	git checkout-index -f -a &&
	rm -f file3 &&
	cat file1 >before &&
	git read-tree -m "$T_BASE" "$T_MOD" &&
	test_cmp before file1 &&
	test_path_is_missing file3
'

test_expect_success 'read-tree -m -u updates index entries' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_MOD=$(cat ../tree_modified) &&
	rm -f .git/index &&
	git read-tree "$T_BASE" &&
	git read-tree -m -u "$T_BASE" "$T_MOD" &&
	git ls-files >actual &&
	grep "file1" actual &&
	grep "file3" actual
'

test_expect_success 'setup: 3-way merge trees' '
	cd repo &&
	git checkout base &&
	echo "ours-change" >file1 &&
	echo "ours-new" >ours-only &&
	git add file1 ours-only &&
	git commit -m "ours" &&
	git tag ours &&
	git rev-parse ours^{tree} >../tree_ours &&
	git checkout base &&
	echo "theirs-change" >file1 &&
	echo "theirs-new" >theirs-only &&
	git add file1 theirs-only &&
	git commit -m "theirs" &&
	git tag theirs &&
	git rev-parse theirs^{tree} >../tree_theirs
'

test_expect_success 'read-tree -m 3-way populates index' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_OURS=$(cat ../tree_ours) &&
	T_THEIRS=$(cat ../tree_theirs) &&
	rm -f .git/index &&
	git read-tree -m "$T_BASE" "$T_OURS" "$T_THEIRS" &&
	git ls-files -s >actual &&
	# 3-way merge should produce index entries
	test -s actual
'

test_expect_success '3-way merge: ours-only appears in index' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_OURS=$(cat ../tree_ours) &&
	T_THEIRS=$(cat ../tree_theirs) &&
	rm -f .git/index &&
	git read-tree -m "$T_BASE" "$T_OURS" "$T_THEIRS" &&
	git ls-files -s >actual &&
	grep "ours-only" actual
'

test_expect_success '3-way merge includes new files from both sides' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_OURS=$(cat ../tree_ours) &&
	T_THEIRS=$(cat ../tree_theirs) &&
	rm -f .git/index &&
	git read-tree -m "$T_BASE" "$T_OURS" "$T_THEIRS" &&
	git ls-files -s >actual &&
	grep "ours-only" actual &&
	grep "theirs-only" actual
'

test_expect_success 'read-tree -m -u with 3-way updates working tree' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_OURS=$(cat ../tree_ours) &&
	T_THEIRS=$(cat ../tree_theirs) &&
	rm -f .git/index &&
	git read-tree "$T_BASE" &&
	git checkout-index -f -a &&
	git read-tree -m -u "$T_BASE" "$T_OURS" "$T_THEIRS" &&
	test -f ours-only
'

test_expect_success 'read-tree -m preserves subdirectory entries' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_MOD=$(cat ../tree_modified) &&
	rm -f .git/index &&
	git read-tree -m "$T_BASE" "$T_MOD" &&
	git ls-files >actual &&
	grep "sub/file2" actual
'

test_expect_success 'single-tree read-tree populates index from tree' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	rm -f .git/index &&
	git read-tree "$T_BASE" &&
	git ls-files >actual &&
	grep "file1" actual &&
	grep "sub/file2" actual &&
	! grep "file3" actual
'

test_expect_success 'read-tree --reset clears and repopulates' '
	cd repo &&
	T_BASE=$(cat ../tree_base) &&
	T_MOD=$(cat ../tree_modified) &&
	rm -f .git/index &&
	git read-tree "$T_MOD" &&
	git read-tree --reset "$T_BASE" &&
	git ls-files >actual &&
	grep "file1" actual &&
	! grep "file3" actual
'

test_done
