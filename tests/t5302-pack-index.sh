#!/bin/sh
# Tests for grit prune-packed, verify-pack, and show-index.
# Ported subset from git/t/t5302-pack-index.sh.

test_description='pack index: prune-packed, verify-pack, show-index'

. ./test-lib.sh

REAL_GIT=${REAL_GIT:-/usr/bin/git}

# ---------------------------------------------------------------------------
# prune-packed basics
# ---------------------------------------------------------------------------

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
# verify-pack tests
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

test_expect_success 'verify-pack -v lists all objects in pack' '
	cd repo_vp &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	obj_count=$(grep -cE "^[0-9a-f]{40}" out) &&
	test "$obj_count" -ge 3
'

test_expect_success 'verify-pack -v output includes offset and size' '
	cd repo_vp &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	grep -E "^[0-9a-f]{40} (commit|tree|blob) [0-9]+ [0-9]+ [0-9]+" out
'

test_expect_success 'verify-pack on pack with multiple objects' '
	rm -rf repo_vp2 &&
	grit init repo_vp2 &&
	cd repo_vp2 &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo file1 >f1.txt &&
	echo file2 >f2.txt &&
	echo file3 >f3.txt &&
	git add f1.txt f2.txt f3.txt &&
	git commit -m "three files" &&
	echo file4 >f4.txt &&
	git add f4.txt &&
	git commit -m "four files" &&
	git repack -a -d &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack "$pack" &&
	git verify-pack -v "$pack" >out &&
	obj_count=$(grep -cE "^[0-9a-f]{40}" out) &&
	test "$obj_count" -ge 7
'

test_expect_success 'verify-pack detects truncated pack' '
	rm -rf repo_vp_trunc &&
	grit init repo_vp_trunc &&
	cd repo_vp_trunc &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo content >f.txt &&
	git add f.txt &&
	git commit -m initial &&
	git repack -a -d &&
	pack=$(echo .git/objects/pack/*.pack) &&
	cp "$pack" "$pack.bak" &&
	dd if="$pack" of="$pack.trunc" bs=1 count=20 2>/dev/null &&
	mv "$pack.trunc" "$pack" &&
	test_must_fail git verify-pack "$pack" 2>err
'

# ---------------------------------------------------------------------------
# show-index tests
# ---------------------------------------------------------------------------

test_expect_success 'show-index reads valid idx and outputs entries' '
	rm -rf repo_si &&
	grit init repo_si &&
	cd repo_si &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo content >file.txt &&
	git add file.txt &&
	git commit -m initial &&
	git repack -a -d &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git show-index <"$idx" >out &&
	test_line_count -gt 0 out
'

test_expect_success 'show-index --object-format=sha1 accepted' '
	cd repo_si &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git show-index --object-format=sha1 <"$idx" >out &&
	test_line_count -gt 0 out
'

test_expect_success 'show-index --object-format=sha256 rejected' '
	cd repo_si &&
	idx=$(echo .git/objects/pack/*.idx) &&
	test_must_fail git show-index --object-format=sha256 <"$idx"
'

test_expect_success 'show-index OIDs match verify-pack OIDs' '
	cd repo_si &&
	idx=$(echo .git/objects/pack/*.idx) &&
	"$REAL_GIT" verify-pack -v "$idx" |
		grep -E "^[0-9a-f]{40}" |
		awk "{print \$1}" | sort >expected_oids &&
	git show-index <"$idx" | awk "{print \$2}" | sort >actual_oids &&
	test_cmp expected_oids actual_oids
'

test_expect_success 'show-index output format: offset OID CRC' '
	cd repo_si &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git show-index <"$idx" >out &&
	while read offset oid rest; do
		test -n "$offset" &&
		test -n "$oid" &&
		echo "$oid" | grep -qE "^[0-9a-f]{40}$"
	done <out
'

test_expect_success 'show-index entries include valid offsets' '
	cd repo_si &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git show-index <"$idx" | awk "{print \$1}" >offsets &&
	while read off; do
		test "$off" -ge 0 || return 1
	done <offsets
'

test_expect_success 'show-index count matches verify-pack count' '
	cd repo_si &&
	idx=$(echo .git/objects/pack/*.idx) &&
	si_count=$(git show-index <"$idx" | wc -l | tr -d " ") &&
	vp_count=$("$REAL_GIT" verify-pack -v "$idx" | grep -cE "^[0-9a-f]{40}") &&
	test "$si_count" = "$vp_count"
'

test_expect_success 'show-index with larger pack' '
	rm -rf repo_si_lg &&
	grit init repo_si_lg &&
	cd repo_si_lg &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	i=1 &&
	while test $i -le 20; do
		echo "content $i" >file_$i.txt &&
		i=$(($i + 1))
	done &&
	git add . &&
	git commit -m "twenty files" &&
	git repack -a -d &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git show-index <"$idx" >out &&
	si_count=$(wc -l <out | tr -d " ") &&
	test "$si_count" -ge 22
'

# ---------------------------------------------------------------------------
# prune-packed with packs
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

test_expect_success 'prune-packed removes all loose objects that are packed' '
	rm -rf repo_pp_all &&
	grit init repo_pp_all &&
	cd repo_pp_all &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo a >a.txt &&
	echo b >b.txt &&
	echo c >c.txt &&
	git add . &&
	git commit -m "three files" &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	test "$loose_before" -gt 0 &&
	git repack -a &&
	grit prune-packed &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes"
'

test_expect_success 'prune-packed leaves objects not in any pack' '
	rm -rf repo_pp_leave &&
	grit init repo_pp_leave &&
	cd repo_pp_leave &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo packed >packed.txt &&
	git add packed.txt &&
	git commit -m "packed commit" &&
	git repack -a -d &&
	EXTRA=$(echo "extra loose" | git hash-object -w --stdin) &&
	EXTRA_FILE=.git/objects/$(echo "$EXTRA" | sed "s/^../&\//") &&
	grit prune-packed &&
	test_path_is_file "$EXTRA_FILE"
'

test_expect_success 'prune-packed with multiple packs' '
	rm -rf repo_pp_multi &&
	grit init repo_pp_multi &&
	cd repo_pp_multi &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo first >first.txt &&
	git add first.txt &&
	git commit -m first &&
	git repack &&
	echo second >second.txt &&
	git add second.txt &&
	git commit -m second &&
	git repack &&
	pack_count=$(ls .git/objects/pack/*.pack 2>/dev/null | wc -l) &&
	test "$pack_count" -ge 2 &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	grit prune-packed &&
	loose_after=$(git count-objects | sed "s/ .*//") &&
	test "$loose_after" -le "$loose_before"
'

test_expect_success 'verify-pack after prune-packed still passes' '
	cd repo_pp_multi &&
	for p in .git/objects/pack/*.pack; do
		git verify-pack "$p" || return 1
	done
'

test_expect_success 'prune-packed and count-objects agree' '
	rm -rf repo_pp_co &&
	grit init repo_pp_co &&
	cd repo_pp_co &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo content >f.txt &&
	git add f.txt &&
	git commit -m initial &&
	git repack -a &&
	grit prune-packed &&
	test "$(git count-objects | sed "s/ .*//")" = "0"
'

test_done
