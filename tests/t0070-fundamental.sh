#!/bin/sh
# Test fundamental object creation and reading operations.

test_description='grit fundamental object operations (hash-object, cat-file, write-tree, commit-tree)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

###########################################################################
# Section 1: Blob creation and retrieval
###########################################################################

test_expect_success 'setup repo' '
	grit init repo &&
	cd repo
'

test_expect_success 'hash-object creates consistent OID for same content' '
	cd repo &&
	echo "test content" >file1.txt &&
	oid1=$(grit hash-object file1.txt) &&
	echo "test content" >file2.txt &&
	oid2=$(grit hash-object file2.txt) &&
	test "$oid1" = "$oid2"
'

test_expect_success 'hash-object produces different OID for different content' '
	cd repo &&
	echo "content A" >a.txt &&
	echo "content B" >b.txt &&
	oid_a=$(grit hash-object a.txt) &&
	oid_b=$(grit hash-object b.txt) &&
	test "$oid_a" != "$oid_b"
'

test_expect_success 'hash-object -w stores the object' '
	cd repo &&
	echo "stored content" >stored.txt &&
	oid=$(grit hash-object -w stored.txt) &&
	test -f ".git/objects/$(echo $oid | cut -c1-2)/$(echo $oid | cut -c3-)"
'

test_expect_success 'cat-file -p retrieves stored blob content' '
	cd repo &&
	echo "hello world" >hw.txt &&
	oid=$(grit hash-object -w hw.txt) &&
	grit cat-file -p "$oid" >actual &&
	echo "hello world" >expect &&
	test_cmp expect actual
'

test_expect_success 'cat-file -t on blob returns blob' '
	cd repo &&
	echo "type test" >tt.txt &&
	oid=$(grit hash-object -w tt.txt) &&
	result=$(grit cat-file -t "$oid") &&
	test "$result" = "blob"
'

test_expect_success 'cat-file -s on blob returns correct size' '
	cd repo &&
	printf "12345" >five.txt &&
	oid=$(grit hash-object -w five.txt) &&
	size=$(grit cat-file -s "$oid") &&
	test "$size" = "5"
'

test_expect_success 'cat-file blob OID prints content' '
	cd repo &&
	echo "blob type test" >bt.txt &&
	oid=$(grit hash-object -w bt.txt) &&
	grit cat-file blob "$oid" >actual &&
	echo "blob type test" >expect &&
	test_cmp expect actual
'

test_expect_success 'hash-object --stdin -w stores from stdin' '
	cd repo &&
	oid=$(printf "stdin data" | grit hash-object --stdin -w) &&
	grit cat-file -p "$oid" >actual &&
	printf "stdin data" >expect &&
	test_cmp expect actual
'

###########################################################################
# Section 2: Tree creation
###########################################################################

test_expect_success 'write-tree on empty index produces empty tree' '
	cd repo &&
	rm -f .git/index &&
	tree=$(grit write-tree) &&
	test "$tree" = "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
'

test_expect_success 'cat-file -t on tree returns tree' '
	cd repo &&
	rm -f .git/index &&
	tree=$(grit write-tree) &&
	result=$(grit cat-file -t "$tree") &&
	test "$result" = "tree"
'

test_expect_success 'write-tree after adding files produces non-empty tree' '
	cd repo &&
	rm -f .git/index &&
	echo "file A" >a.txt &&
	echo "file B" >b.txt &&
	grit update-index --add a.txt b.txt &&
	tree=$(grit write-tree) &&
	test "$tree" != "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
'

test_expect_success 'ls-tree on written tree shows added files' '
	cd repo &&
	rm -f .git/index &&
	echo "alpha" >alpha.txt &&
	grit update-index --add alpha.txt &&
	tree=$(grit write-tree) &&
	grit ls-tree "$tree" >actual &&
	grep "alpha.txt" actual
'

test_expect_success 'write-tree is deterministic for same index content' '
	cd repo &&
	rm -f .git/index &&
	echo "deterministic" >det.txt &&
	grit update-index --add det.txt &&
	tree1=$(grit write-tree) &&
	rm -f .git/index &&
	echo "deterministic" >det.txt &&
	grit update-index --add det.txt &&
	tree2=$(grit write-tree) &&
	test "$tree1" = "$tree2"
'

test_expect_success 'write-tree with subdirectory creates subtree' '
	cd repo &&
	rm -f .git/index &&
	mkdir -p sub &&
	echo "in sub" >sub/file.txt &&
	grit update-index --add sub/file.txt &&
	tree=$(grit write-tree) &&
	grit ls-tree "$tree" >actual &&
	grep "tree" actual &&
	grep "sub" actual
'

###########################################################################
# Section 3: Commit creation
###########################################################################

test_expect_success 'commit-tree creates a commit object' '
	cd repo &&
	rm -f .git/index &&
	echo "for commit" >c.txt &&
	grit update-index --add c.txt &&
	tree=$(grit write-tree) &&
	commit=$(echo "test commit" | grit commit-tree "$tree") &&
	test -n "$commit"
'

test_expect_success 'cat-file -t on commit returns commit' '
	cd repo &&
	rm -f .git/index &&
	echo "commit type" >ct.txt &&
	grit update-index --add ct.txt &&
	tree=$(grit write-tree) &&
	commit=$(echo "a commit" | grit commit-tree "$tree") &&
	result=$(grit cat-file -t "$commit") &&
	test "$result" = "commit"
'

test_expect_success 'cat-file -p on commit shows tree and message' '
	cd repo &&
	rm -f .git/index &&
	echo "msg test" >msg.txt &&
	grit update-index --add msg.txt &&
	tree=$(grit write-tree) &&
	commit=$(echo "my message" | grit commit-tree "$tree") &&
	grit cat-file -p "$commit" >actual &&
	grep "^tree $tree" actual &&
	grep "my message" actual
'

test_expect_success 'commit-tree -p creates commit with parent' '
	cd repo &&
	rm -f .git/index &&
	echo "parent test" >p.txt &&
	grit update-index --add p.txt &&
	tree=$(grit write-tree) &&
	parent=$(echo "parent commit" | grit commit-tree "$tree") &&
	echo "updated" >p.txt &&
	grit update-index --add p.txt &&
	tree2=$(grit write-tree) &&
	child=$(echo "child commit" | grit commit-tree "$tree2" -p "$parent") &&
	grit cat-file -p "$child" >actual &&
	grep "^parent $parent" actual
'

test_expect_success 'commit-tree with multiple -p creates merge commit' '
	cd repo &&
	rm -f .git/index &&
	echo "merge base" >m.txt &&
	grit update-index --add m.txt &&
	tree=$(grit write-tree) &&
	p1=$(echo "parent 1" | grit commit-tree "$tree") &&
	p2=$(echo "parent 2" | grit commit-tree "$tree") &&
	merge=$(echo "merge commit" | grit commit-tree "$tree" -p "$p1" -p "$p2") &&
	grit cat-file -p "$merge" >actual &&
	grep "^parent $p1" actual &&
	grep "^parent $p2" actual
'

###########################################################################
# Section 4: Object integrity
###########################################################################

test_expect_success 'cat-file -e on existing object succeeds' '
	cd repo &&
	echo "exists" >ex.txt &&
	oid=$(grit hash-object -w ex.txt) &&
	grit cat-file -e "$oid"
'

test_expect_success 'cat-file -e on missing object fails' '
	cd repo &&
	test_must_fail grit cat-file -e 0000000000000000000000000000000000000000
'

test_expect_success 'cat-file with invalid OID fails' '
	cd repo &&
	test_must_fail grit cat-file -t invalidhash 2>err
'

test_expect_success 'hash-object on binary content works' '
	cd repo &&
	printf "\x00\x01\x02\x03" >binary.bin &&
	oid=$(grit hash-object -w binary.bin) &&
	test -n "$oid" &&
	grit cat-file -t "$oid" >actual &&
	test "$(cat actual)" = "blob"
'

test_expect_success 'hash-object on large file works' '
	cd repo &&
	dd if=/dev/zero bs=1024 count=100 2>/dev/null >large.bin &&
	oid=$(grit hash-object -w large.bin) &&
	size=$(grit cat-file -s "$oid") &&
	test "$size" = "102400"
'

test_expect_success 'hash-object on empty file returns known OID' '
	cd repo &&
	: >empty &&
	oid=$(grit hash-object empty) &&
	test "$oid" = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
'

test_expect_success 'update-index --cacheinfo adds entry without file' '
	cd repo &&
	rm -f .git/index &&
	blob=$(echo "virtual" | grit hash-object --stdin -w) &&
	grit update-index --add --cacheinfo "100644,$blob,virtual.txt" &&
	grit ls-files -s >actual &&
	grep "virtual.txt" actual
'

test_done
