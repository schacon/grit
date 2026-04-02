#!/bin/sh
# Cache-tree: write-tree performance, tree reuse, index/tree consistency.

test_description='grit cache-tree and write-tree reuse'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo with files' '
	grit init repo &&
	cd repo &&
	grit config user.email "author@example.com" &&
	grit config user.name "A U Thor" &&
	echo "a" >a.txt &&
	echo "b" >b.txt &&
	mkdir -p dir &&
	echo "c" >dir/c.txt &&
	grit add a.txt b.txt dir/c.txt &&
	test_tick &&
	grit commit -m "initial"
'

test_expect_success 'write-tree returns same OID as HEAD tree' '
	cd repo &&
	tree=$(grit write-tree) &&
	head_tree=$(grit rev-parse HEAD^{tree}) &&
	test "$tree" = "$head_tree"
'

test_expect_success 'repeated write-tree returns identical OID' '
	cd repo &&
	tree1=$(grit write-tree) &&
	tree2=$(grit write-tree) &&
	test "$tree1" = "$tree2"
'

test_expect_success 'write-tree after no-op update-index is stable' '
	cd repo &&
	tree_before=$(grit write-tree) &&
	grit update-index --refresh &&
	tree_after=$(grit write-tree) &&
	test "$tree_before" = "$tree_after"
'

test_expect_success 'write-tree changes after modifying a file' '
	cd repo &&
	tree_before=$(grit write-tree) &&
	echo "modified" >a.txt &&
	grit update-index --add a.txt &&
	tree_after=$(grit write-tree) &&
	test "$tree_before" != "$tree_after"
'

test_expect_success 'write-tree after restoring original content returns original tree' '
	cd repo &&
	original_tree=$(grit rev-parse HEAD^{tree}) &&
	echo "a" >a.txt &&
	grit update-index --add a.txt &&
	restored_tree=$(grit write-tree) &&
	test "$original_tree" = "$restored_tree"
'

test_expect_success 'write-tree after adding new file differs' '
	cd repo &&
	tree_before=$(grit write-tree) &&
	echo "new" >new.txt &&
	grit update-index --add new.txt &&
	tree_after=$(grit write-tree) &&
	test "$tree_before" != "$tree_after"
'

test_expect_success 'write-tree after removing added file returns to original' '
	cd repo &&
	tree_before=$(grit write-tree) &&
	grit update-index --force-remove new.txt &&
	tree_after=$(grit write-tree) &&
	original=$(grit rev-parse HEAD^{tree}) &&
	test "$tree_after" = "$original"
'

test_expect_success 'write-tree after only subdir change has different root but same sibling subtree' '
	cd repo &&
	tree_before=$(grit write-tree) &&
	grit ls-tree "$tree_before" >entries_before &&
	echo "modified c" >dir/c.txt &&
	grit update-index --add dir/c.txt &&
	tree_after=$(grit write-tree) &&
	test "$tree_before" != "$tree_after" &&
	grit ls-tree "$tree_after" >entries_after &&
	blob_a_before=$(awk "\$4==\"a.txt\" {print \$3}" entries_before) &&
	blob_a_after=$(awk "\$4==\"a.txt\" {print \$3}" entries_after) &&
	test "$blob_a_before" = "$blob_a_after"
'

test_expect_success 'restore dir/c.txt to original' '
	cd repo &&
	echo "c" >dir/c.txt &&
	grit update-index --add dir/c.txt
'

test_expect_success 'write-tree with many files' '
	cd repo &&
	mkdir -p many &&
	for i in $(seq 1 50); do
		echo "file $i" >many/f$i.txt
	done &&
	grit add many/ &&
	tree=$(grit write-tree) &&
	grit ls-tree "$tree" many >entries &&
	test_line_count = 1 entries &&
	many_oid=$(awk "{print \$3}" entries) &&
	grit ls-tree "$many_oid" >many_entries &&
	test_line_count = 50 many_entries
'

test_expect_success 'write-tree is idempotent with many files' '
	cd repo &&
	tree1=$(grit write-tree) &&
	tree2=$(grit write-tree) &&
	test "$tree1" = "$tree2"
'

test_expect_success 'modifying one of many files only changes root and parent subtree' '
	cd repo &&
	tree_before=$(grit write-tree) &&
	echo "modified" >many/f25.txt &&
	grit update-index --add many/f25.txt &&
	tree_after=$(grit write-tree) &&
	test "$tree_before" != "$tree_after" &&
	grit ls-tree "$tree_before" >before &&
	grit ls-tree "$tree_after" >after &&
	dir_before=$(awk "\$4==\"dir\" {print \$3}" before) &&
	dir_after=$(awk "\$4==\"dir\" {print \$3}" after) &&
	test "$dir_before" = "$dir_after"
'

test_expect_success 'write-tree after read-tree reset matches' '
	cd repo &&
	head_tree=$(grit rev-parse HEAD^{tree}) &&
	grit read-tree --reset "$head_tree" &&
	tree=$(grit write-tree) &&
	test "$tree" = "$head_tree"
'

test_expect_success 'commit with many files for further tests' '
	cd repo &&
	grit add many/ a.txt b.txt dir/c.txt &&
	test_tick &&
	grit commit -m "with many"
'

test_expect_success 'write-tree after read-tree of another commit' '
	cd repo &&
	second_tree=$(grit rev-parse HEAD^{tree}) &&
	first_tree=$(grit rev-parse HEAD~1^{tree}) &&
	grit read-tree --reset "$first_tree" &&
	tree=$(grit write-tree) &&
	test "$tree" = "$first_tree" &&
	grit read-tree --reset "$second_tree" &&
	tree=$(grit write-tree) &&
	test "$tree" = "$second_tree"
'

test_expect_success 'fresh index write-tree matches after re-adding all files' '
	cd repo &&
	expected=$(grit rev-parse HEAD^{tree}) &&
	rm -f .git/index &&
	grit add a.txt b.txt dir/c.txt many/ &&
	actual=$(grit write-tree) &&
	test "$expected" = "$actual"
'

test_expect_success 'write-tree with nested directories' '
	cd repo &&
	mkdir -p deep/a/b/c &&
	echo "leaf" >deep/a/b/c/leaf.txt &&
	grit add deep/ &&
	tree=$(grit write-tree) &&
	grit ls-tree -r "$tree" deep >actual &&
	grep "deep/a/b/c/leaf.txt" actual
'

test_expect_success 'write-tree after removing directory entries' '
	cd repo &&
	tree_with=$(grit write-tree) &&
	grit update-index --force-remove deep/a/b/c/leaf.txt &&
	tree_without=$(grit write-tree) &&
	test "$tree_with" != "$tree_without" &&
	grit ls-tree -r "$tree_without" >entries &&
	! grep "deep" entries
'

test_expect_success 'empty subdirectory does not appear in tree' '
	cd repo &&
	mkdir -p emptydir &&
	tree=$(grit write-tree) &&
	grit ls-tree "$tree" >entries &&
	! grep "emptydir" entries
'

test_expect_success 'write-tree with only executables' '
	cd repo &&
	rm -f .git/index &&
	echo "#!/bin/sh" >exec1.sh &&
	echo "#!/bin/sh" >exec2.sh &&
	chmod +x exec1.sh exec2.sh &&
	grit update-index --add exec1.sh exec2.sh &&
	tree=$(grit write-tree) &&
	grit ls-tree "$tree" >entries &&
	test_line_count = 2 entries &&
	grep "100755" entries
'

test_expect_success 'write-tree preserves file modes' '
	cd repo &&
	grit ls-tree "$(grit write-tree)" >entries &&
	mode1=$(awk "\$4==\"exec1.sh\" {print \$1}" entries) &&
	test "$mode1" = "100755"
'

test_expect_success 'write-tree after chmod change reflects new mode' '
	cd repo &&
	rm -f .git/index &&
	chmod 644 exec1.sh &&
	grit update-index --add exec1.sh exec2.sh &&
	tree=$(grit write-tree) &&
	grit ls-tree "$tree" >entries &&
	mode1=$(awk "\$4==\"exec1.sh\" {print \$1}" entries) &&
	test "$mode1" = "100644"
'

test_expect_success 'large tree with 200 entries' '
	cd repo &&
	rm -f .git/index &&
	for i in $(seq 1 200); do
		echo "content $i" >file_$i.txt
	done &&
	grit add file_*.txt &&
	tree=$(grit write-tree) &&
	grit ls-tree "$tree" >entries &&
	test_line_count = 200 entries
'

test_expect_success 'write-tree and ls-tree roundtrip for large tree' '
	cd repo &&
	tree=$(grit write-tree) &&
	grit ls-tree "$tree" >ls_out &&
	grit mktree <ls_out >mktree_sha &&
	test "$tree" = "$(cat mktree_sha)"
'

test_expect_success 'write-tree after index-info load' '
	cd repo &&
	rm -f .git/index &&
	blob=$(echo "hello" | grit hash-object -w --stdin) &&
	grit update-index --index-info <<-EOF &&
	100644 $blob 0	loaded.txt
	EOF
	tree=$(grit write-tree) &&
	grit ls-tree "$tree" >actual &&
	grep "loaded.txt" actual
'

test_expect_success 'write-tree speed: 200 files completes in reasonable time' '
	cd repo &&
	rm -f .git/index &&
	for i in $(seq 1 200); do
		echo "content $i" >speed_$i.txt
	done &&
	grit add speed_*.txt &&
	start=$(date +%s) &&
	grit write-tree >/dev/null &&
	end=$(date +%s) &&
	elapsed=$((end - start)) &&
	test "$elapsed" -lt 10
'

test_done
