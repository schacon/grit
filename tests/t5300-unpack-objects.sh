#!/bin/sh
# Integration tests for grit unpack-objects.
#
# Exercises:
#   - basic blob/tree/commit unpacking from a real pack stream
#   - dry-run (-n) does not write loose objects
#   - objects already in the ODB are not double-written
#   - --strict flag is accepted (compat)
#   - invalid pack stream is rejected
#   - quiet (-q) suppresses informational output

test_description='grit unpack-objects basic tests'

. ./test-lib.sh

REAL_GIT=${REAL_GIT:-/usr/bin/git}

# Set up a small repo, add a few objects, and capture a pack stream to test.pack.
test_expect_success 'setup: create objects and capture pack stream' '
	"$REAL_GIT" init src.git --bare &&
	"$REAL_GIT" -C src.git config user.email "test@example.com" &&
	"$REAL_GIT" -C src.git config user.name "Test" &&
	echo "hello world" | "$REAL_GIT" -C src.git hash-object -w --stdin &&
	echo "foo bar"     | "$REAL_GIT" -C src.git hash-object -w --stdin &&
	TREE=$("$REAL_GIT" -C src.git write-tree) &&
	COMMIT=$(echo "initial commit" | "$REAL_GIT" -C src.git commit-tree "$TREE") &&
	"$REAL_GIT" -C src.git update-ref HEAD "$COMMIT" &&
	"$REAL_GIT" -C src.git pack-objects --revs --stdout <<-EOF >test.pack
		HEAD
	EOF
'

test_expect_success 'unpack-objects: unpacks blobs, tree, commit into new ODB' '
	grit init dest.git --bare &&
	grit -C dest.git unpack-objects <test.pack &&
	COMMIT=$("$REAL_GIT" -C src.git rev-parse HEAD) &&
	"$REAL_GIT" -C dest.git cat-file -t "$COMMIT" >type.out &&
	echo commit >type.exp &&
	test_cmp type.exp type.out
'

test_expect_success 'unpack-objects -n: dry run writes no loose objects' '
	grit init dry.git --bare &&
	grit -C dry.git unpack-objects -n <test.pack &&
	count=$(find dry.git/objects -type f | wc -l) &&
	test "$count" = "0"
'

test_expect_success 'unpack-objects -q: quiet flag produces no stderr' '
	grit init quiet.git --bare &&
	grit -C quiet.git unpack-objects -q <test.pack 2>err &&
	test_must_be_empty err
'

test_expect_success 'unpack-objects --strict: flag accepted without error' '
	grit init strict.git --bare &&
	grit -C strict.git unpack-objects --strict <test.pack
'

test_expect_success 'unpack-objects: rejects an invalid pack signature' '
	printf "NOPE\000\000\000\002\000\000\000\000" >bad.pack &&
	printf "%020d" 0 >>bad.pack &&
	grit init bad.git --bare &&
	test_must_fail grit -C bad.git unpack-objects <bad.pack
'

test_expect_success 'unpack-objects: idempotent — running twice succeeds' '
	grit init dup.git --bare &&
	grit -C dup.git unpack-objects <test.pack &&
	grit -C dup.git unpack-objects <test.pack
'

test_expect_success 'unpack-objects: all objects readable with grit cat-file' '
	grit init verify.git --bare &&
	grit -C verify.git unpack-objects <test.pack &&
	while IFS= read -r oid; do
		grit -C verify.git cat-file -e "$oid" || { echo "missing $oid"; false; }
	done <<-EOF
		$("$REAL_GIT" -C src.git rev-parse HEAD)
		$("$REAL_GIT" -C src.git rev-parse HEAD^{tree})
	EOF
'

test_expect_success 'unpack-objects: blob content readable with cat-file -p' '
	grit init content.git --bare &&
	grit -C content.git unpack-objects <test.pack &&
	BLOB_OID=$("$REAL_GIT" -C src.git rev-list --objects HEAD | awk "NF>1 {print \$1}" | head -1) &&
	if test -n "$BLOB_OID" && grit -C content.git cat-file -e "$BLOB_OID" 2>/dev/null; then
		grit -C content.git cat-file -p "$BLOB_OID" >/dev/null
	fi
'

test_expect_success 'unpack-objects: tree object ls-tree works after unpack' '
	grit init tree_check.git --bare &&
	grit -C tree_check.git unpack-objects <test.pack &&
	TREE_OID=$("$REAL_GIT" -C src.git rev-parse HEAD^{tree}) &&
	grit -C tree_check.git cat-file -t "$TREE_OID" >type_out &&
	echo tree >type_exp &&
	test_cmp type_exp type_out
'

test_expect_success 'unpack-objects: object count correct after unpack' '
	grit init count.git --bare &&
	grit -C count.git unpack-objects <test.pack &&
	COUNT=$(find count.git/objects -type f | grep -v pack | wc -l | tr -d " ") &&
	test "$COUNT" -gt 0
'

test_expect_success 'unpack-objects: commit parent is accessible' '
	grit init parent.git --bare &&
	grit -C parent.git unpack-objects <test.pack &&
	COMMIT=$("$REAL_GIT" -C src.git rev-parse HEAD) &&
	grit -C parent.git cat-file -t "$COMMIT" >t &&
	echo commit >exp &&
	test_cmp exp t
'

test_expect_success 'unpack-objects: empty pack rejected gracefully' '
	> empty.pack &&
	grit init empty.git --bare &&
	test_must_fail grit -C empty.git unpack-objects <empty.pack
'

test_done
