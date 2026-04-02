#!/bin/sh
# Test grit behaviour with corrupted, missing, and malformed objects.

test_description='grit object corruption and error handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

###########################################################################
# Section 1: Setup
###########################################################################

test_expect_success 'setup base repository' '
	grit init repo &&
	cd repo &&
	git config user.email "test@test.com" &&
	git config user.name "Test" &&
	echo "hello world" >file.txt &&
	grit add file.txt &&
	grit commit -m "initial commit"
'

###########################################################################
# Section 2: Missing objects
###########################################################################

test_expect_success 'cat-file fails on nonexistent object' '
	cd repo &&
	test_must_fail grit cat-file -p 0000000000000000000000000000000000000001 2>err &&
	grep -i "not found" err
'

test_expect_success 'cat-file -t fails on nonexistent object' '
	cd repo &&
	test_must_fail grit cat-file -t 0000000000000000000000000000000000000001 2>err &&
	grep -i "not found" err
'

test_expect_success 'cat-file -s fails on nonexistent object' '
	cd repo &&
	test_must_fail grit cat-file -s 0000000000000000000000000000000000000001 2>err &&
	grep -i "not found" err
'

test_expect_success 'cat-file -e fails on nonexistent object' '
	cd repo &&
	test_must_fail grit cat-file -e 0000000000000000000000000000000000000001
'

test_expect_success 'cat-file -e succeeds on existing object' '
	cd repo &&
	OID=$(grit rev-parse HEAD) &&
	grit cat-file -e "$OID"
'

###########################################################################
# Section 3: Corrupted loose objects
###########################################################################

test_expect_success 'corrupt a loose blob object' '
	cd repo &&
	BLOB=$(grit hash-object -w file.txt) &&
	echo "$BLOB" >blob_oid &&
	OBJPATH=".git/objects/$(echo $BLOB | cut -c1-2)/$(echo $BLOB | cut -c3-)" &&
	echo "corrupted-data" >"$OBJPATH"
'

test_expect_success 'cat-file -p fails on corrupted blob' '
	cd repo &&
	BLOB=$(cat blob_oid) &&
	test_must_fail grit cat-file -p "$BLOB" 2>err &&
	grep -i -e "corrupt" -e "zlib" -e "inflate" err
'

test_expect_success 'cat-file -t fails on corrupted blob' '
	cd repo &&
	BLOB=$(cat blob_oid) &&
	test_must_fail grit cat-file -t "$BLOB" 2>err &&
	grep -i -e "corrupt" -e "zlib" -e "inflate" err
'

test_expect_success 'cat-file -s fails on corrupted blob' '
	cd repo &&
	BLOB=$(cat blob_oid) &&
	test_must_fail grit cat-file -s "$BLOB" 2>err &&
	grep -i -e "corrupt" -e "zlib" -e "inflate" err
'

test_expect_success 'checkout-index fails on corrupted blob' '
	cd repo &&
	test_must_fail grit checkout-index --force file.txt 2>err &&
	grep -i -e "corrupt" -e "zlib" -e "inflate" -e "error" err
'

###########################################################################
# Section 4: Re-adding after corruption
###########################################################################

test_expect_success 'hash-object -w can re-create a corrupted blob' '
	cd repo &&
	BLOB_BEFORE=$(cat blob_oid) &&
	OBJPATH=".git/objects/$(echo $BLOB_BEFORE | cut -c1-2)/$(echo $BLOB_BEFORE | cut -c3-)" &&
	rm -f "$OBJPATH" &&
	BLOB_AFTER=$(grit hash-object -w file.txt) &&
	test "$BLOB_BEFORE" = "$BLOB_AFTER"
'

test_expect_success 'cat-file works after re-creating blob' '
	cd repo &&
	BLOB=$(cat blob_oid) &&
	grit cat-file -p "$BLOB" >actual &&
	echo "hello world" >expect &&
	test_cmp expect actual
'

###########################################################################
# Section 5: Corrupted commit object
###########################################################################

test_expect_success 'setup: corrupt commit object' '
	cd repo &&
	COMMIT=$(grit rev-parse HEAD) &&
	echo "$COMMIT" >commit_oid &&
	OBJPATH=".git/objects/$(echo $COMMIT | cut -c1-2)/$(echo $COMMIT | cut -c3-)" &&
	cp "$OBJPATH" "$OBJPATH.bak" &&
	echo "garbage" >"$OBJPATH"
'

test_expect_success 'cat-file -p fails on corrupted commit' '
	cd repo &&
	COMMIT=$(cat commit_oid) &&
	test_must_fail grit cat-file -p "$COMMIT" 2>err &&
	grep -i -e "corrupt" -e "zlib" -e "inflate" err
'

test_expect_success 'log fails with corrupted commit' '
	cd repo &&
	test_must_fail grit log 2>err &&
	grep -i -e "corrupt" -e "zlib" -e "inflate" -e "error" err
'

test_expect_success 'restore corrupted commit' '
	cd repo &&
	COMMIT=$(cat commit_oid) &&
	OBJPATH=".git/objects/$(echo $COMMIT | cut -c1-2)/$(echo $COMMIT | cut -c3-)" &&
	cp "$OBJPATH.bak" "$OBJPATH"
'

test_expect_success 'cat-file works after restoring commit' '
	cd repo &&
	COMMIT=$(cat commit_oid) &&
	grit cat-file -t "$COMMIT" >actual &&
	echo "commit" >expect &&
	test_cmp expect actual
'

###########################################################################
# Section 6: Corrupted tree object
###########################################################################

test_expect_success 'setup: corrupt tree object' '
	cd repo &&
	TREE=$(grit cat-file -p HEAD | grep "^tree " | cut -d" " -f2) &&
	echo "$TREE" >tree_oid &&
	OBJPATH=".git/objects/$(echo $TREE | cut -c1-2)/$(echo $TREE | cut -c3-)" &&
	cp "$OBJPATH" "$OBJPATH.bak" &&
	echo "trashed" >"$OBJPATH"
'

test_expect_success 'cat-file -p fails on corrupted tree' '
	cd repo &&
	TREE=$(cat tree_oid) &&
	test_must_fail grit cat-file -p "$TREE" 2>err &&
	grep -i -e "corrupt" -e "zlib" -e "inflate" err
'

test_expect_success 'ls-tree fails with corrupted tree' '
	cd repo &&
	TREE=$(cat tree_oid) &&
	test_must_fail grit ls-tree "$TREE" 2>err &&
	grep -i -e "corrupt" -e "zlib" -e "inflate" -e "error" err
'

test_expect_success 'restore corrupted tree' '
	cd repo &&
	TREE=$(cat tree_oid) &&
	OBJPATH=".git/objects/$(echo $TREE | cut -c1-2)/$(echo $TREE | cut -c3-)" &&
	cp "$OBJPATH.bak" "$OBJPATH"
'

###########################################################################
# Section 7: Truncated objects
###########################################################################

test_expect_success 'truncated blob fails gracefully' '
	cd repo &&
	echo "new content" >new.txt &&
	BLOB=$(grit hash-object -w new.txt) &&
	OBJPATH=".git/objects/$(echo $BLOB | cut -c1-2)/$(echo $BLOB | cut -c3-)" &&
	dd if="$OBJPATH" of="$OBJPATH.trunc" bs=1 count=2 2>/dev/null &&
	mv "$OBJPATH.trunc" "$OBJPATH" &&
	test_must_fail grit cat-file -p "$BLOB" 2>err
'

test_expect_success 'empty object file fails gracefully' '
	cd repo &&
	echo "another" >another.txt &&
	BLOB=$(grit hash-object -w another.txt) &&
	OBJPATH=".git/objects/$(echo $BLOB | cut -c1-2)/$(echo $BLOB | cut -c3-)" &&
	: >"$OBJPATH" &&
	test_must_fail grit cat-file -p "$BLOB" 2>err
'

###########################################################################
# Section 8: Invalid object IDs
###########################################################################

test_expect_success 'cat-file rejects short hex IDs' '
	cd repo &&
	test_must_fail grit cat-file -p abc 2>err
'

test_expect_success 'cat-file rejects non-hex characters' '
	cd repo &&
	test_must_fail grit cat-file -p gggggggggggggggggggggggggggggggggggggggg 2>err
'

test_expect_success 'rev-parse rejects garbage input' '
	cd repo &&
	test_must_fail grit rev-parse not-a-valid-ref 2>err
'

###########################################################################
# Section 9: Batch cat-file with missing objects
###########################################################################

test_expect_success 'cat-file --batch handles missing objects' '
	cd repo &&
	echo "0000000000000000000000000000000000000099" |
	grit cat-file --batch >actual 2>&1 &&
	grep -i "missing" actual
'

test_expect_success 'cat-file --batch-check handles missing objects' '
	cd repo &&
	echo "0000000000000000000000000000000000000099" |
	grit cat-file --batch-check >actual 2>&1 &&
	grep -i "missing" actual
'

test_expect_success 'cat-file --batch-check with existing object' '
	cd repo &&
	OID=$(grit rev-parse HEAD) &&
	echo "$OID" |
	grit cat-file --batch-check >actual &&
	grep "commit" actual
'

test_done
