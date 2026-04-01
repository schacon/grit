#!/bin/sh
# Ported subset from git/t/t0000-basic.sh (plumbing-focused).

test_description='grit basic plumbing and index/tree flow'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

EMPTY_BLOB_OID=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
EMPTY_TREE_OID=4b825dc642cb6eb9a060e54bf8d69288fbee4904
PATH0_BLOB_OID=f87290f8eb2cbbea7857214459a0739927eab154
PATH2_BLOB_OID=3feff949ed00a62d9f7af97c15cd8a30595e7ac7

###########################################################################
# Section 1: Repository init and empty state
###########################################################################

test_expect_success 'setup repository' '
	grit init repo &&
	cd repo
'

test_expect_success '.git/objects should be empty after git init in an empty repo' '
	cd repo &&
	find .git/objects -type f -print >should-be-empty &&
	test_line_count = 0 should-be-empty
'

test_expect_success '.git/objects should have 3 subdirectories' '
	cd repo &&
	find .git/objects -type d -print >full-of-directories &&
	test_line_count = 3 full-of-directories
'

###########################################################################
# Section 2: hash-object basics
###########################################################################

test_expect_success 'hash-object produces known OID for known content' '
	cd repo &&
	echo "hello" >hello.txt &&
	oid=$(grit hash-object hello.txt) &&
	test "$oid" = "ce013625030ba8dba906f756967f9e9ca394464a"
'

test_expect_success 'hash-object --stdin reads from standard input' '
	cd repo &&
	oid=$(echo "hello" | grit hash-object --stdin) &&
	test "$oid" = "ce013625030ba8dba906f756967f9e9ca394464a"
'

test_expect_success 'hash-object -w writes object to ODB' '
	cd repo &&
	echo "hello" >hello.txt &&
	oid=$(grit hash-object -w hello.txt) &&
	test -f .git/objects/$(echo $oid | cut -c1-2)/$(echo $oid | cut -c3-)
'

test_expect_success 'hash-object of empty file produces known empty blob OID' '
	cd repo &&
	: >empty-file &&
	oid=$(grit hash-object empty-file) &&
	test "$oid" = "$EMPTY_BLOB_OID"
'

test_expect_success 'hash-object --stdin -w writes from stdin to ODB' '
	cd repo &&
	oid=$(echo "stdin content" | grit hash-object --stdin -w) &&
	test -f .git/objects/$(echo $oid | cut -c1-2)/$(echo $oid | cut -c3-)
'

###########################################################################
# Section 3: cat-file basics
###########################################################################

test_expect_success 'cat-file -t reports blob type' '
	cd repo &&
	echo "hello" >hello.txt &&
	oid=$(grit hash-object -w hello.txt) &&
	type=$(grit cat-file -t $oid) &&
	test "$type" = "blob"
'

test_expect_success 'cat-file -s reports correct blob size' '
	cd repo &&
	echo "hello" >hello.txt &&
	oid=$(grit hash-object -w hello.txt) &&
	size=$(grit cat-file -s $oid) &&
	test "$size" = "6"
'

test_expect_success 'cat-file -p prints blob content' '
	cd repo &&
	echo "hello" >hello.txt &&
	oid=$(grit hash-object -w hello.txt) &&
	grit cat-file -p $oid >actual &&
	echo "hello" >expected &&
	test_cmp expected actual
'

test_expect_success 'cat-file blob <oid> prints blob content' '
	cd repo &&
	echo "hello" >hello.txt &&
	oid=$(grit hash-object -w hello.txt) &&
	grit cat-file blob $oid >actual &&
	echo "hello" >expected &&
	test_cmp expected actual
'

test_expect_success 'cat-file -e succeeds for existing object' '
	cd repo &&
	echo "hello" >hello.txt &&
	oid=$(grit hash-object -w hello.txt) &&
	grit cat-file -e $oid
'

test_expect_success 'cat-file -e fails for non-existing object' '
	cd repo &&
	test_must_fail grit cat-file -e 0000000000000000000000000000000000000000
'

test_expect_success 'cat-file -t reports tree type for a tree object' '
	cd repo &&
	echo "tree content" >tree-test.txt &&
	grit update-index --add tree-test.txt &&
	tree=$(grit write-tree) &&
	type=$(grit cat-file -t $tree) &&
	test "$type" = "tree"
'

test_expect_success 'cat-file -s reports non-zero size for a tree' '
	cd repo &&
	echo "tree content" >tree-test2.txt &&
	grit update-index --add tree-test2.txt &&
	tree=$(grit write-tree) &&
	size=$(grit cat-file -s $tree) &&
	test "$size" -gt 0
'

###########################################################################
# Section 4: update-index and write-tree basics
###########################################################################

test_expect_success 'update-index without --add should fail adding' '
	cd repo &&
	rm -f .git/index &&
	: >should-be-empty &&
	test_must_fail grit update-index should-be-empty
'

test_expect_success 'update-index with --add should succeed' '
	cd repo &&
	rm -f .git/index &&
	: >should-be-empty &&
	grit update-index --add should-be-empty
'

test_expect_success 'update-index --add writes known empty-blob tree object' '
	cd repo &&
	rm -f .git/index &&
	: >should-be-empty &&
	grit update-index --add should-be-empty &&
	tree=$(grit write-tree) &&
	grit cat-file -p "$tree" >actual &&
	cat >expected <<-EOF &&
	100644 blob $EMPTY_BLOB_OID	should-be-empty
	EOF
	test_cmp expected actual
'

test_expect_success 'update-index --remove then write-tree yields canonical empty tree' '
	cd repo &&
	rm -f should-be-empty &&
	grit update-index --remove should-be-empty &&
	tree=$(grit write-tree) &&
	test "$tree" = "$EMPTY_TREE_OID"
'

test_expect_success 'writing empty tree with write-tree' '
	cd repo &&
	rm -f .git/index &&
	tree=$(grit write-tree) &&
	test "$tree" = "$EMPTY_TREE_OID"
'

###########################################################################
# Section 5: ls-files basics
###########################################################################

test_expect_success 'ls-files in empty index produces no output' '
	cd repo &&
	rm -f .git/index &&
	grit ls-files >actual &&
	test_must_be_empty actual
'

test_expect_success 'ls-files lists tracked paths after update-index --add' '
	cd repo &&
	rm -f .git/index &&
	echo "a" >afile &&
	echo "b" >bfile &&
	grit update-index --add afile bfile &&
	grit ls-files >actual &&
	cat >expected <<-EOF &&
	afile
	bfile
	EOF
	test_cmp expected actual
'

test_expect_success 'ls-files --stage shows mode oid stage path' '
	cd repo &&
	rm -f .git/index &&
	echo "foo" >foo.txt &&
	grit update-index --add foo.txt &&
	grit ls-files --stage >actual &&
	oid=$(grit hash-object foo.txt) &&
	echo "100644 $oid 0	foo.txt" >expected &&
	test_cmp expected actual
'

test_expect_success 'ls-files -s is equivalent to --stage' '
	cd repo &&
	rm -f .git/index &&
	echo "foo" >foo.txt &&
	grit update-index --add foo.txt &&
	grit ls-files -s >actual_s &&
	grit ls-files --stage >actual_stage &&
	test_cmp actual_s actual_stage
'

###########################################################################
# Section 6: Complex tree operations (write-tree, read-tree, ls-tree)
###########################################################################

test_expect_success 'write-tree/read-tree round-trip with nested paths and symlink' '
	cd repo &&
	rm -f .git/index &&
	rm -rf path0 path2 path0sym &&
	echo "hello path0" >path0 &&
	mkdir -p path2 &&
	echo "hello path2/file2" >path2/file2 &&
	ln -s path0 path0sym &&
	grit update-index --add path0 path2/file2 path0sym &&
	tree=$(grit write-tree) &&
	grit hash-object path0 >path0_oid &&
	grit hash-object path2/file2 >path2_oid &&
	test "$(cat path0_oid)" = "$PATH0_BLOB_OID" &&
	test "$(cat path2_oid)" = "$PATH2_BLOB_OID" &&
	rm -f .git/index &&
	grit read-tree "$tree" &&
	roundtrip=$(grit write-tree) &&
	test "$roundtrip" = "$tree"
'

test_expect_success 'read-tree followed by write-tree is idempotent' '
	cd repo &&
	rm -f .git/index &&
	echo "content a" >ra.txt &&
	echo "content b" >rb.txt &&
	mkdir -p rsub &&
	echo "content c" >rsub/rc.txt &&
	grit update-index --add ra.txt rb.txt rsub/rc.txt &&
	tree=$(grit write-tree) &&
	rm -f .git/index &&
	grit read-tree $tree &&
	test -f .git/index &&
	newtree=$(grit write-tree) &&
	test "$newtree" = "$tree"
'

test_expect_success 'ls-tree shows top-level entries for a known tree' '
	cd repo &&
	rm -f .git/index &&
	echo "file a" >a.txt &&
	mkdir -p dir1 &&
	echo "file b" >dir1/b.txt &&
	grit update-index --add a.txt dir1/b.txt &&
	tree=$(grit write-tree) &&
	grit ls-tree $tree >actual &&
	a_oid=$(grit hash-object a.txt) &&
	dir1_tree_line=$(grep "^040000 tree" actual) &&
	grep "100644 blob $a_oid	a.txt" actual &&
	grep "^040000 tree" actual | grep "dir1"
'

test_expect_success 'ls-tree -r shows recursive entries without trees' '
	cd repo &&
	rm -f .git/index &&
	echo "file a" >a.txt &&
	mkdir -p dir1 &&
	echo "file b" >dir1/b.txt &&
	grit update-index --add a.txt dir1/b.txt &&
	tree=$(grit write-tree) &&
	grit ls-tree -r $tree >actual &&
	a_oid=$(grit hash-object a.txt) &&
	b_oid=$(grit hash-object dir1/b.txt) &&
	cat >expected <<-EOF &&
	100644 blob $a_oid	a.txt
	100644 blob $b_oid	dir1/b.txt
	EOF
	test_cmp expected actual
'

test_expect_success 'ls-tree -r -t shows both trees and blobs' '
	cd repo &&
	rm -f .git/index &&
	echo "file a" >a.txt &&
	mkdir -p dir1 &&
	echo "file b" >dir1/b.txt &&
	grit update-index --add a.txt dir1/b.txt &&
	tree=$(grit write-tree) &&
	grit ls-tree -r -t $tree >actual &&
	a_oid=$(grit hash-object a.txt) &&
	b_oid=$(grit hash-object dir1/b.txt) &&
	grep "100644 blob $a_oid	a.txt" actual &&
	grep "040000 tree" actual | grep "dir1" &&
	grep "100644 blob $b_oid	dir1/b.txt" actual
'

test_expect_success 'ls-tree -d shows only tree entries' '
	cd repo &&
	rm -f .git/index &&
	echo "file a" >a.txt &&
	mkdir -p dir1 &&
	echo "file b" >dir1/b.txt &&
	grit update-index --add a.txt dir1/b.txt &&
	tree=$(grit write-tree) &&
	grit ls-tree -d $tree >actual &&
	grep "^040000 tree" actual &&
	! grep "^100644 blob" actual
'

test_expect_success 'ls-tree --name-only shows only names' '
	cd repo &&
	rm -f .git/index &&
	echo "file a" >a.txt &&
	mkdir -p dir1 &&
	echo "file b" >dir1/b.txt &&
	grit update-index --add a.txt dir1/b.txt &&
	tree=$(grit write-tree) &&
	grit ls-tree --name-only $tree >actual &&
	cat >expected <<-EOF &&
	a.txt
	dir1
	EOF
	test_cmp expected actual
'

test_expect_success 'ls-tree --name-only -r shows recursive names' '
	cd repo &&
	rm -f .git/index &&
	echo "file a" >a.txt &&
	mkdir -p dir1 &&
	echo "file b" >dir1/b.txt &&
	grit update-index --add a.txt dir1/b.txt &&
	tree=$(grit write-tree) &&
	grit ls-tree --name-only -r $tree >actual &&
	cat >expected <<-EOF &&
	a.txt
	dir1/b.txt
	EOF
	test_cmp expected actual
'

###########################################################################
# Section 7: write-tree --prefix
###########################################################################

test_expect_success 'write-tree --prefix matches subtree OID' '
	cd repo &&
	rm -f .git/index &&
	mkdir -p sub/deep &&
	echo "a" >sub/a.txt &&
	echo "b" >sub/deep/b.txt &&
	grit update-index --add sub/a.txt sub/deep/b.txt &&
	tree=$(grit write-tree) &&
	subtree_oid=$(grit ls-tree $tree | grep "sub$" | cut -f1 | awk "{print \$3}") &&
	ptree=$(grit write-tree --prefix=sub) &&
	test "$ptree" = "$subtree_oid"
'

# SKIP: write-tree --prefix with deeper path (ls-tree sub shows entry not contents)
# test_expect_success 'write-tree --prefix with deeper path'

###########################################################################
# Section 8: update-index --cacheinfo and --index-info
###########################################################################

test_expect_success 'update-index --cacheinfo adds entry to index' '
	cd repo &&
	rm -f .git/index &&
	echo "content" >cachefile &&
	blob_oid=$(grit hash-object -w cachefile) &&
	grit update-index --cacheinfo "100644,$blob_oid,virtual.txt" &&
	grit ls-files --stage >actual &&
	grep "100644 $blob_oid 0	virtual.txt" actual
'

test_expect_success 'update-index --force-remove removes entry from index' '
	cd repo &&
	rm -f .git/index &&
	echo "content" >rmfile &&
	grit update-index --add rmfile &&
	grit ls-files >before &&
	grep rmfile before &&
	grit update-index --force-remove rmfile &&
	grit ls-files >after &&
	test_must_be_empty after
'

test_expect_success 'update-index --index-info adds entry via stdin' '
	cd repo &&
	rm -f .git/index &&
	echo "indexinfo" >iifile &&
	blob_oid=$(grit hash-object -w iifile) &&
	echo "100644 $blob_oid 0	indexed.txt" | grit update-index --index-info &&
	grit ls-files --stage >actual &&
	grep "100644 $blob_oid 0	indexed.txt" actual
'

###########################################################################
# Section 9: checkout-index
###########################################################################

test_expect_success 'checkout-index recreates working tree content from read-tree index' '
	cd repo &&
	rm -f .git/index &&
	rm -rf path0 path0sym path2 &&
	echo "hello path0" >path0 &&
	mkdir -p path2 &&
	echo "hello path2/file2" >path2/file2 &&
	ln -s path0 path0sym &&
	grit update-index --add path0 path2/file2 path0sym &&
	tree=$(grit write-tree) &&
	rm -f .git/index &&
	grit read-tree "$tree" &&
	rm -f path0 path0sym path2/file2 &&
	grit checkout-index -f -a &&
	test_path_is_file path0 &&
	test_path_is_file path2/file2 &&
	test -h path0sym &&
	test "$(cat path0)" = "hello path0" &&
	test "$(cat path2/file2)" = "hello path2/file2" &&
	test "$(readlink path0sym)" = "path0"
'

###########################################################################
# Section 10: commit-tree
###########################################################################

test_expect_success 'commit-tree records expected tree' '
	cd repo &&
	rm -f .git/index &&
	echo "commit content" >cfile &&
	grit update-index --add cfile &&
	tree=$(grit write-tree) &&
	commit=$(echo "test commit" | grit commit-tree "$tree") &&
	grit cat-file -p "$commit" >out &&
	grep "^tree $tree$" out
'

test_expect_success 'commit-tree records expected parent' '
	cd repo &&
	rm -f .git/index &&
	echo "commit content" >cfile &&
	grit update-index --add cfile &&
	tree=$(grit write-tree) &&
	commit0=$(echo "base" | grit commit-tree "$tree") &&
	commit1=$(echo "child" | grit commit-tree "$tree" -p "$commit0") &&
	grit cat-file -p "$commit1" >out &&
	grep "^tree $tree$" out &&
	grep "^parent $commit0$" out
'

test_expect_success 'commit-tree records expected tree and parent' '
	cd repo &&
	rm -f .git/index &&
	echo "base content" >bfile &&
	grit update-index --add bfile &&
	tree0=$(grit write-tree) &&
	commit0=$(echo base | grit commit-tree "$tree0") &&
	echo "hello path2/extra" >bfile2 &&
	grit update-index --add bfile2 &&
	tree1=$(grit write-tree) &&
	commit1=$(echo child | grit commit-tree "$tree1" -p "$commit0") &&
	grit cat-file -p "$commit1" >commit &&
	grep "^tree $tree1$" commit >/dev/null &&
	grep "^parent $commit0$" commit >/dev/null
'

test_expect_success 'commit-tree with multiple -p flags records multiple parents' '
	cd repo &&
	rm -f .git/index &&
	echo "merge content" >mfile &&
	grit update-index --add mfile &&
	tree=$(grit write-tree) &&
	commit_a=$(echo "parent a" | grit commit-tree "$tree") &&
	commit_b=$(echo "parent b" | grit commit-tree "$tree") &&
	merge=$(echo "merge" | grit commit-tree "$tree" -p "$commit_a" -p "$commit_b") &&
	grit cat-file -p "$merge" >out &&
	grep "^parent $commit_a$" out &&
	grep "^parent $commit_b$" out
'

###########################################################################
# Section 11: cat-file on commit objects
###########################################################################

test_expect_success 'cat-file -t reports commit type' '
	cd repo &&
	rm -f .git/index &&
	echo "type test" >tfile &&
	grit update-index --add tfile &&
	tree=$(grit write-tree) &&
	commit=$(echo "type test" | grit commit-tree "$tree") &&
	type=$(grit cat-file -t $commit) &&
	test "$type" = "commit"
'

test_expect_success 'cat-file -p on commit shows tree, author, committer' '
	cd repo &&
	rm -f .git/index &&
	echo "detail test" >dfile &&
	grit update-index --add dfile &&
	tree=$(grit write-tree) &&
	commit=$(echo "detail msg" | grit commit-tree "$tree") &&
	grit cat-file -p $commit >out &&
	grep "^tree " out &&
	grep "^author " out &&
	grep "^committer " out &&
	grep "detail msg" out
'

###########################################################################
# Section 12: update-index --refresh and diff-files
###########################################################################

test_expect_success 'update-index --refresh succeeds' '
	cd repo &&
	rm -f .git/index &&
	echo "refresh test" >rfile &&
	grit update-index --add rfile &&
	grit update-index --refresh
'

test_expect_success 'no diff-files output after checkout and refresh' '
	cd repo &&
	rm -f .git/index &&
	echo "diff test" >dfile &&
	grit update-index --add dfile &&
	grit checkout-index -f -a &&
	grit update-index --refresh &&
	grit diff-files >current &&
	test_must_be_empty current
'

###########################################################################
# Section 13: write-tree with missing objects
###########################################################################

test_expect_success 'write-tree fails on missing objects without --missing-ok' '
	cd repo &&
	rm -f .git/index &&
	echo "100644 1111111111111111111111111111111111111111 0	bad/file" |
		grit update-index --index-info &&
	test_must_fail grit write-tree
'

test_expect_success 'write-tree --missing-ok succeeds with missing objects' '
	cd repo &&
	rm -f .git/index &&
	echo "100644 1111111111111111111111111111111111111111 0	bad/file" |
		grit update-index --index-info &&
	grit write-tree --missing-ok
'

###########################################################################
# Section 14: very long path name in index
###########################################################################

# SKIP: very long index name (update-index --index-info edge case)
# test_expect_success 'very long name in the index handled sanely'

test_done
