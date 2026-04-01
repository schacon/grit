#!/bin/sh
# Ported subset from git/t/t0000-basic.sh (plumbing-focused).

test_description='grit basic plumbing and index/tree flow'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

EMPTY_BLOB_OID=e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
EMPTY_TREE_OID=4b825dc642cb6eb9a060e54bf8d69288fbee4904
PATH0_BLOB_OID=f87290f8eb2cbbea7857214459a0739927eab154
PATH2_BLOB_OID=3feff949ed00a62d9f7af97c15cd8a30595e7ac7

test_expect_success 'setup repository' '
	grit init repo &&
	cd repo
'

test_expect_success 'update-index --add writes known empty-blob tree object' '
	cd repo &&
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

test_expect_success 'write-tree/read-tree round-trip with nested paths and symlink' '
	cd repo &&
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

test_expect_success 'checkout-index recreates working tree content from read-tree index' '
	cd repo &&
	rm -f path0 path0sym path2/file2 &&
	grit checkout-index -f -a &&
	test_path_is_file path0 &&
	test_path_is_file path2/file2 &&
	test -h path0sym &&
	test "$(cat path0)" = "hello path0" &&
	test "$(cat path2/file2)" = "hello path2/file2" &&
	test "$(readlink path0sym)" = "path0"
'

test_expect_success 'commit-tree records expected tree and parent' '
	cd repo &&
	tree0=$(grit write-tree) &&
	commit0=$(echo base | grit commit-tree "$tree0") &&
	echo "hello path2/extra" >path2/extra &&
	grit update-index --add path2/extra &&
	tree1=$(grit write-tree) &&
	commit1=$(echo child | grit commit-tree "$tree1" -p "$commit0") &&
	grit cat-file -p "$commit1" >commit &&
	grep "^tree $tree1\$" commit >/dev/null &&
	grep "^parent $commit0\$" commit >/dev/null
'

test_done
