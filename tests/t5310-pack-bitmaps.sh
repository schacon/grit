#!/bin/sh
#
# Tests for pack files, verify-pack, repack, and their interactions.
# Covers: repack, verify-pack -v/-s, count-objects, gc,
# show-index, prune-packed.
#
# NOTE: grit commit cannot read packed objects yet, so we do all
# commits before repacking or use /usr/bin/git for commits
# after repack.

test_description='grit pack bitmaps and verify-pack interaction'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# ---------------------------------------------------------------------------
# Setup: repo with several commits for pack testing
# ---------------------------------------------------------------------------
test_expect_success 'setup repository with 5 commits' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	for i in 1 2 3 4 5; do
		echo "content $i" >file$i.txt &&
		git add file$i.txt &&
		git commit -m "commit $i"
	done
'

# ---------------------------------------------------------------------------
# Loose objects before packing
# ---------------------------------------------------------------------------
test_expect_success 'count-objects reports objects before repack' '
	cd repo &&
	grit count-objects >../out 2>&1 &&
	grep "objects" ../out
'

test_expect_success 'count-objects -v shows detailed info' '
	cd repo &&
	grit count-objects -v >../out 2>&1 &&
	grep "^size:" ../out &&
	grep "^in-pack:" ../out &&
	grep "^packs:" ../out
'

test_expect_success 'count-objects -v shows count field' '
	cd repo &&
	grit count-objects -v >../out 2>&1 &&
	grep "^count:" ../out
'

test_expect_success 'loose objects exist before repack' '
	cd repo &&
	grit count-objects -v >../out 2>&1 &&
	count=$(grep "^count:" ../out | sed "s/count: *//") &&
	test "$count" -gt 0
'

# ---------------------------------------------------------------------------
# repack
# ---------------------------------------------------------------------------
test_expect_success 'grit repack creates pack file' '
	cd repo &&
	grit repack -a -d &&
	ls .git/objects/pack/*.pack >../packs &&
	test_line_count = 1 ../packs
'

test_expect_success 'repack creates index file' '
	cd repo &&
	ls .git/objects/pack/*.idx >../idxs &&
	test_line_count = 1 ../idxs
'

test_expect_success 'count-objects -v shows 0 loose after repack' '
	cd repo &&
	grit count-objects -v >../out 2>&1 &&
	count=$(grep "^count:" ../out | sed "s/count: *//") &&
	test "$count" = "0"
'

test_expect_success 'count-objects -v shows objects in pack' '
	cd repo &&
	grit count-objects -v >../out 2>&1 &&
	inpack=$(grep "^in-pack:" ../out | sed "s/in-pack: *//") &&
	test "$inpack" -gt 0
'

test_expect_success 'count-objects -v shows 1 pack after repack' '
	cd repo &&
	grit count-objects -v >../out 2>&1 &&
	packs=$(grep "^packs:" ../out | sed "s/packs: *//") &&
	test "$packs" = "1"
'

# ---------------------------------------------------------------------------
# verify-pack
# ---------------------------------------------------------------------------
test_expect_success 'verify-pack succeeds on valid pack' '
	cd repo &&
	grit verify-pack .git/objects/pack/*.idx
'

test_expect_success 'verify-pack -v shows object listing' '
	cd repo &&
	grit verify-pack -v .git/objects/pack/*.idx >../out &&
	grep "commit" ../out &&
	grep "blob" ../out &&
	grep "tree" ../out
'

test_expect_success 'verify-pack -v shows chain length histogram' '
	cd repo &&
	grit verify-pack -v .git/objects/pack/*.idx >../out &&
	grep "chain length" ../out
'

test_expect_success 'verify-pack -v shows ok status' '
	cd repo &&
	grit verify-pack -v .git/objects/pack/*.idx >../out &&
	grep "ok" ../out
'

test_expect_success 'verify-pack -s shows stat-only (chain histogram)' '
	cd repo &&
	grit verify-pack -s .git/objects/pack/*.idx >../out &&
	grep "chain length" ../out
'

test_expect_success 'verify-pack -v lists at least 10 objects' '
	cd repo &&
	grit verify-pack -v .git/objects/pack/*.idx >../out &&
	obj_count=$(grep -c "^[0-9a-f]\{40\}" ../out) &&
	test "$obj_count" -ge 10
'

test_expect_success 'verify-pack -v shows 5 commit objects' '
	cd repo &&
	grit verify-pack -v .git/objects/pack/*.idx >../out &&
	commit_count=$(grep -c " commit " ../out) &&
	test "$commit_count" = "5"
'

test_expect_success 'verify-pack -v shows 5 blob objects' '
	cd repo &&
	grit verify-pack -v .git/objects/pack/*.idx >../out &&
	blob_count=$(grep -c " blob " ../out) &&
	test "$blob_count" = "5"
'

test_expect_success 'verify-pack with .pack file instead of .idx' '
	cd repo &&
	grit verify-pack .git/objects/pack/*.pack
'

# ---------------------------------------------------------------------------
# show-index
# ---------------------------------------------------------------------------
test_expect_success 'show-index reads pack index' '
	cd repo &&
	idx=$(ls .git/objects/pack/*.idx) &&
	grit show-index <"$idx" >../out &&
	test -s ../out
'

test_expect_success 'show-index lists all objects' '
	cd repo &&
	idx=$(ls .git/objects/pack/*.idx) &&
	grit show-index <"$idx" >../out &&
	line_count=$(wc -l <../out | tr -d " ") &&
	test "$line_count" -ge 10
'

test_expect_success 'show-index output contains hex OIDs' '
	cd repo &&
	idx=$(ls .git/objects/pack/*.idx) &&
	grit show-index <"$idx" >../out &&
	grep "[0-9a-f]\{40\}" ../out
'

# ---------------------------------------------------------------------------
# gc
# ---------------------------------------------------------------------------
test_expect_success 'gc succeeds' '
	cd repo &&
	grit gc
'

test_expect_success 'gc leaves valid pack' '
	cd repo &&
	ls .git/objects/pack/*.idx >../gc_idxs 2>/dev/null &&
	test -s ../gc_idxs &&
	while read idx; do
		grit verify-pack "$idx"
	done <../gc_idxs
'

# ---------------------------------------------------------------------------
# Fresh repo: 8 commits, then repack, then verify
# ---------------------------------------------------------------------------
test_expect_success 'setup repo2: 8 commits then repack' '
	git init repo2 &&
	cd repo2 &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&

	for i in 1 2 3 4 5 6 7 8; do
		echo "data $i" >f$i.txt &&
		git add f$i.txt &&
		git commit -m "c$i"
	done &&
	grit repack -a -d
'

test_expect_success 'verify-pack after repack of 8-commit repo' '
	cd repo2 &&
	grit verify-pack .git/objects/pack/*.idx
'

test_expect_success 'verify-pack -v shows 8 commits in repo2' '
	cd repo2 &&
	grit verify-pack -v .git/objects/pack/*.idx >../out &&
	commit_count=$(grep -c " commit " ../out) &&
	test "$commit_count" = "8"
'

test_expect_success 'verify-pack -v shows 8 blobs in repo2' '
	cd repo2 &&
	grit verify-pack -v .git/objects/pack/*.idx >../out &&
	blob_count=$(grep -c " blob " ../out) &&
	test "$blob_count" = "8"
'

test_expect_success 'count-objects -v shows 0 loose in repo2' '
	cd repo2 &&
	grit count-objects -v >../out 2>&1 &&
	count=$(grep "^count:" ../out | sed "s/count: *//") &&
	test "$count" = "0"
'

test_expect_success 'show-index for repo2' '
	cd repo2 &&
	idx=$(ls .git/objects/pack/*.idx) &&
	grit show-index <"$idx" >../out &&
	line_count=$(wc -l <../out | tr -d " ") &&
	test "$line_count" -ge 16
'

# ---------------------------------------------------------------------------
# Adding commits after repack using real git, then grit repack again
# ---------------------------------------------------------------------------
test_expect_success 'add commits after repack with /usr/bin/git' '
	cd repo2 &&
	for i in 9 10; do
		echo "extra $i" >f$i.txt &&
		/usr/bin/git add f$i.txt &&
		/usr/bin/git commit -m "c$i"
	done
'

test_expect_success 'new loose objects exist after /usr/bin/git commits' '
	cd repo2 &&
	grit count-objects -v >../out 2>&1 &&
	count=$(grep "^count:" ../out | sed "s/count: *//") &&
	test "$count" -gt 0
'

test_expect_success 'grit repack collects new loose objects' '
	cd repo2 &&
	grit repack -a -d &&
	grit count-objects -v >../out 2>&1 &&
	count=$(grep "^count:" ../out | sed "s/count: *//") &&
	test "$count" = "0"
'

test_expect_success 'verify-pack shows 10 commits after re-repack' '
	cd repo2 &&
	grit verify-pack -v .git/objects/pack/*.idx >../out &&
	commit_count=$(grep -c " commit " ../out) &&
	test "$commit_count" = "10"
'

# ---------------------------------------------------------------------------
# prune-packed
# ---------------------------------------------------------------------------
test_expect_success 'setup: fresh repo for prune-packed' '
	git init repo3 &&
	cd repo3 &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&

	echo "data" >file.txt &&
	git add file.txt &&
	git commit -m "initial" &&
	grit repack -a -d
'

test_expect_success 'prune-packed removes loose objects already in pack' '
	cd repo3 &&
	grit prune-packed &&
	grit count-objects -v >../out 2>&1 &&
	count=$(grep "^count:" ../out | sed "s/count: *//") &&
	test "$count" = "0"
'

test_expect_success 'verify-pack still valid after prune-packed' '
	cd repo3 &&
	grit verify-pack .git/objects/pack/*.idx
'

test_done
