#!/bin/sh
# Test read-tree with unmerged entries and conflict scenarios.

test_description='grit read-tree with unmerged index entries'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

###########################################################################
# Section 1: Setup helper
###########################################################################

test_expect_success 'setup: create base, ours, and theirs trees' '
	grit init repo &&
	cd repo &&

	# Base tree: shared, common, base-only
	echo "base content" >shared &&
	echo "common" >common &&
	echo "base only" >base-only &&
	rm -f .git/index &&
	grit update-index --add shared common base-only &&
	tree_base=$(grit write-tree) &&
	echo "$tree_base" >../tree_base &&

	# Ours: modify shared, keep common, add ours-new
	echo "ours content" >shared &&
	echo "common" >common &&
	echo "ours new" >ours-new &&
	rm -f .git/index &&
	grit update-index --add shared common ours-new &&
	tree_ours=$(grit write-tree) &&
	echo "$tree_ours" >../tree_ours &&

	# Theirs: modify shared differently, keep common, add theirs-new
	echo "theirs content" >shared &&
	echo "common" >common &&
	echo "theirs new" >theirs-new &&
	rm -f .git/index &&
	grit update-index --add shared common theirs-new &&
	tree_theirs=$(grit write-tree) &&
	echo "$tree_theirs" >../tree_theirs
'

###########################################################################
# Section 2: Three-way merge creates unmerged entries
###########################################################################

test_expect_success '3-way merge marks divergent file as unmerged' '
	cd repo &&
	rm -f .git/index &&
	grit read-tree -m "$(cat ../tree_base)" "$(cat ../tree_ours)" "$(cat ../tree_theirs)" &&
	grit ls-files -u >unmerged &&
	grep "shared" unmerged
'

test_expect_success 'unmerged file has stages 1, 2, and 3' '
	cd repo &&
	grit ls-files -u >unmerged &&
	grep "1.shared" unmerged &&
	grep "2.shared" unmerged &&
	grep "3.shared" unmerged
'

test_expect_success 'stage 1 blob matches base content' '
	cd repo &&
	grit ls-files -u >unmerged &&
	base_oid=$(grep "1.shared" unmerged | awk "{print \$2}") &&
	grit cat-file -p "$base_oid" >actual &&
	echo "base content" >expect &&
	test_cmp expect actual
'

test_expect_success 'stage 2 blob matches ours content' '
	cd repo &&
	grit ls-files -u >unmerged &&
	ours_oid=$(grep "2.shared" unmerged | awk "{print \$2}") &&
	grit cat-file -p "$ours_oid" >actual &&
	echo "ours content" >expect &&
	test_cmp expect actual
'

test_expect_success 'stage 3 blob matches theirs content' '
	cd repo &&
	grit ls-files -u >unmerged &&
	theirs_oid=$(grep "3.shared" unmerged | awk "{print \$2}") &&
	grit cat-file -p "$theirs_oid" >actual &&
	echo "theirs content" >expect &&
	test_cmp expect actual
'

test_expect_success 'common file is resolved at stage 0' '
	cd repo &&
	grit ls-files -s common >actual &&
	grep " 0	common" actual
'

test_expect_success 'common file is NOT in unmerged list' '
	cd repo &&
	grit ls-files -u >unmerged &&
	! grep "common" unmerged
'

###########################################################################
# Section 3: Deleted paths in merge
###########################################################################

test_expect_success 'base-only file deleted in both sides is gone' '
	cd repo &&
	grit ls-files -s >all &&
	! grep "base-only" all
'

test_expect_success 'ours-new file resolves to stage 0' '
	cd repo &&
	grit ls-files -s >all &&
	grep "ours-new" all >actual &&
	grep " 0" actual
'

test_expect_success 'theirs-new file resolves to stage 0' '
	cd repo &&
	grit ls-files -s >all &&
	grep "theirs-new" all >actual &&
	grep " 0" actual
'

###########################################################################
# Section 4: read-tree --reset clears unmerged state
###########################################################################

test_expect_success 'read-tree --reset clears unmerged entries' '
	cd repo &&
	grit read-tree --reset "$(cat ../tree_base)" &&
	grit ls-files -u >unmerged &&
	test_must_be_empty unmerged
'

test_expect_success 'after reset, all files at stage 0' '
	cd repo &&
	grit ls-files -s >staged &&
	! grep -v " 0	" staged
'

###########################################################################
# Section 5: Two-way merge scenarios
###########################################################################

test_expect_success 'setup: two divergent trees from same base' '
	cd repo &&
	rm -f .git/index &&
	echo "two-way base" >tw.txt &&
	echo "stable" >stable.txt &&
	grit update-index --add tw.txt stable.txt &&
	tree_tw_base=$(grit write-tree) &&
	echo "$tree_tw_base" >../tree_tw_base &&

	rm -f .git/index &&
	echo "two-way modified" >tw.txt &&
	echo "stable" >stable.txt &&
	grit update-index --add tw.txt stable.txt &&
	tree_tw_mod=$(grit write-tree) &&
	echo "$tree_tw_mod" >../tree_tw_mod
'

test_expect_success '2-way merge with matching index succeeds' '
	cd repo &&
	rm -f .git/index &&
	grit read-tree "$(cat ../tree_tw_base)" &&
	grit read-tree -m "$(cat ../tree_tw_base)" "$(cat ../tree_tw_mod)" &&
	grit ls-files -s tw.txt >actual &&
	grep " 0	tw.txt" actual
'

test_expect_success '2-way merge: unchanged file remains at stage 0' '
	cd repo &&
	grit ls-files -s stable.txt >actual &&
	grep " 0	stable.txt" actual
'

###########################################################################
# Section 6: Multiple conflicting files
###########################################################################

test_expect_success 'setup: trees with multiple conflicts' '
	cd repo &&

	rm -f .git/index &&
	echo "base A" >fileA &&
	echo "base B" >fileB &&
	echo "base C" >fileC &&
	grit update-index --add fileA fileB fileC &&
	tree_mc_base=$(grit write-tree) &&
	echo "$tree_mc_base" >../tree_mc_base &&

	rm -f .git/index &&
	echo "ours A" >fileA &&
	echo "ours B" >fileB &&
	echo "base C" >fileC &&
	grit update-index --add fileA fileB fileC &&
	tree_mc_ours=$(grit write-tree) &&
	echo "$tree_mc_ours" >../tree_mc_ours &&

	rm -f .git/index &&
	echo "theirs A" >fileA &&
	echo "theirs B" >fileB &&
	echo "base C" >fileC &&
	grit update-index --add fileA fileB fileC &&
	tree_mc_theirs=$(grit write-tree) &&
	echo "$tree_mc_theirs" >../tree_mc_theirs
'

test_expect_success '3-way merge with multiple conflicts shows all unmerged' '
	cd repo &&
	rm -f .git/index &&
	grit read-tree -m "$(cat ../tree_mc_base)" "$(cat ../tree_mc_ours)" "$(cat ../tree_mc_theirs)" &&
	grit ls-files -u >unmerged &&
	grep "fileA" unmerged &&
	grep "fileB" unmerged
'

test_expect_success 'non-conflicting file in multi-conflict merge is resolved' '
	cd repo &&
	grit ls-files -s fileC >actual &&
	grep " 0	fileC" actual
'

test_expect_success 'each conflicting file has 3 unmerged entries' '
	cd repo &&
	grit ls-files -u >unmerged &&
	for f in fileA fileB; do
		count=$(grep "$f" unmerged | wc -l | tr -d " ") &&
		test "$count" = "3" || return 1
	done
'

test_expect_success 'read-tree --reset after multi-conflict clears all' '
	cd repo &&
	grit read-tree --reset "$(cat ../tree_mc_base)" &&
	grit ls-files -u >unmerged &&
	test_must_be_empty unmerged &&
	grit ls-files -s >staged &&
	count=$(wc -l <staged | tr -d " ") &&
	test "$count" = "3"
'

###########################################################################
# Section 7: Identical changes resolve cleanly
###########################################################################

test_expect_success 'setup: identical modification on both sides' '
	cd repo &&

	rm -f .git/index &&
	echo "old" >same.txt &&
	grit update-index --add same.txt &&
	tree_same_base=$(grit write-tree) &&
	echo "$tree_same_base" >../tree_same_base &&

	rm -f .git/index &&
	echo "new" >same.txt &&
	grit update-index --add same.txt &&
	tree_same_both=$(grit write-tree) &&
	echo "$tree_same_both" >../tree_same_both
'

test_expect_success 'identical changes on both sides resolve to stage 0' '
	cd repo &&
	rm -f .git/index &&
	grit read-tree -m "$(cat ../tree_same_base)" "$(cat ../tree_same_both)" "$(cat ../tree_same_both)" &&
	grit ls-files -s same.txt >actual &&
	grep " 0	same.txt" actual &&
	grit ls-files -u >unmerged &&
	! grep "same.txt" unmerged
'

test_done
