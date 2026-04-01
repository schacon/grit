#!/bin/sh
# Tests for grit prune-packed: remove loose objects that are already in a pack.

test_description='prune-packed removes objects already in pack files'

. ./test-lib.sh

REAL_GIT=${REAL_GIT:-/usr/bin/git}

test_expect_success 'setup: create loose object' '
	git init repo &&
	cd repo &&
	BLOB=$(echo "hello prune" | git hash-object -w --stdin) &&
	BLOB_FILE=.git/objects/$(echo "$BLOB" | sed "s/^../&\//") &&
	test_path_is_file "$BLOB_FILE"
'

test_expect_success 'prune-packed with no packs leaves loose object intact' '
	cd repo &&
	BLOB=$(echo "hello prune" | git hash-object --stdin) &&
	BLOB_FILE=.git/objects/$(echo "$BLOB" | sed "s/^../&\//") &&
	grit prune-packed &&
	test_path_is_file "$BLOB_FILE"
'

test_expect_success 'prune-packed --dry-run with no packs produces no output' '
	cd repo &&
	grit prune-packed --dry-run >out &&
	test_must_be_empty out
'

test_expect_success 'prune-packed -n is alias for --dry-run' '
	cd repo &&
	grit prune-packed -n >out &&
	test_must_be_empty out
'

test_expect_success 'prune-packed -q runs without error' '
	cd repo &&
	grit prune-packed -q
'

# ---------------------------------------------------------------------------
# Additional verify-pack and show-index tests
# ---------------------------------------------------------------------------

test_expect_success 'verify-pack on a valid pack passes' '
	rm -rf repo_vp &&
	grit init repo_vp &&
	cd repo_vp &&
	echo hello >a.txt &&
	git add a.txt &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	git commit -m initial &&
	git repack -a -d &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack "$pack"
'

test_expect_success 'verify-pack -v shows object details' '
	cd repo_vp &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	grep "commit" out &&
	grep "tree" out &&
	grep "blob" out
'

test_expect_success 'verify-pack accepts .idx path' '
	cd repo_vp &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git verify-pack "$idx"
'

test_expect_success 'verify-pack -s shows stat summary' '
	cd repo_vp &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -s "$pack" >out &&
	test -s out
'

test_expect_success 'verify-pack fails on nonexistent file' '
	cd repo_vp &&
	test_must_fail git verify-pack nonexistent.pack 2>err
'

test_expect_success 'show-index reads valid idx and outputs entries' '
	cd repo_vp &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git show-index <"$idx" >out &&
	test_line_count -gt 0 out
'

test_expect_success 'show-index --object-format=sha1 accepted' '
	cd repo_vp &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git show-index --object-format=sha1 <"$idx" >out &&
	test_line_count -gt 0 out
'

test_expect_success 'show-index --object-format=sha256 rejected' '
	cd repo_vp &&
	idx=$(echo .git/objects/pack/*.idx) &&
	test_must_fail git show-index --object-format=sha256 <"$idx"
'

test_expect_success 'show-index OIDs match verify-pack OIDs' '
	cd repo_vp &&
	idx=$(echo .git/objects/pack/*.idx) &&
	"$REAL_GIT" verify-pack -v "$idx" |
		grep -E "^[0-9a-f]{40}" |
		awk "{print \$1}" | sort >expected_oids &&
	git show-index <"$idx" | awk "{print \$2}" | sort >actual_oids &&
	test_cmp expected_oids actual_oids
'

# ---------------------------------------------------------------------------
# Additional prune-packed tests
# ---------------------------------------------------------------------------

test_expect_success 'prune-packed removes loose objects already in pack' '
	rm -rf repo_pp &&
	grit init repo_pp &&
	cd repo_pp &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo "pack me" >f.txt &&
	git add f.txt &&
	git commit -m initial &&
	BLOB=$(git hash-object f.txt) &&
	BLOB_FILE=.git/objects/$(echo "$BLOB" | sed "s/^../&\//") &&
	git repack -a &&
	test_path_is_file "$BLOB_FILE" &&
	grit prune-packed &&
	test_path_is_missing "$BLOB_FILE"
'

test_expect_success 'prune-packed --dry-run lists but does not remove' '
	rm -rf repo_ppd &&
	grit init repo_ppd &&
	cd repo_ppd &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo "dry run me" >f.txt &&
	git add f.txt &&
	git commit -m initial &&
	git repack -a -d &&
	BLOB=$(echo extra_dry | git hash-object -w --stdin) &&
	BLOB_FILE=.git/objects/$(echo "$BLOB" | sed "s/^../&\//") &&
	git repack -a &&
	test_path_is_file "$BLOB_FILE" &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	grit prune-packed --dry-run >out &&
	loose_after=$(git count-objects | sed "s/ .*//") &&
	test "$loose_before" = "$loose_after"
'

test_expect_success 'prune-packed -n does not remove objects' '
	cd repo_ppd &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	grit prune-packed -n >out2 &&
	loose_after=$(git count-objects | sed "s/ .*//") &&
	test "$loose_before" = "$loose_after"
'

test_expect_success 'prune-packed has no effect when no packs exist' '
	rm -rf repo_ppnp &&
	grit init repo_ppnp &&
	cd repo_ppnp &&
	BLOB=$(echo "no pack" | git hash-object -w --stdin) &&
	BLOB_FILE=.git/objects/$(echo "$BLOB" | sed "s/^../&\//") &&
	grit prune-packed &&
	test_path_is_file "$BLOB_FILE"
'

test_done
