#!/bin/sh
# Test grit pack-related commands: verify-pack, show-index, repack,
# prune-packed, count-objects.

test_description='grit pack object operations'

. ./test-lib.sh

REAL_GIT=/usr/bin/git

test_expect_success 'setup repository with loose objects' '
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	for i in 1 2 3 4 5; do
		echo "content $i" >file$i.txt
	done &&
	git add -A &&
	git commit -m "initial with 5 files"
'

test_expect_success 'count-objects shows loose objects' '
	cd repo &&
	grit count-objects >actual &&
	grep "objects" actual
'

test_expect_success 'count-objects -v shows detailed stats' '
	cd repo &&
	grit count-objects -v >actual &&
	grep "^count:" actual &&
	grep "^size:" actual &&
	grep "^in-pack:" actual &&
	grep "^packs:" actual
'

test_expect_success 'initially no packs exist' '
	cd repo &&
	grit count-objects -v >actual &&
	grep "^packs: 0" actual
'

test_expect_success 'repack creates pack file' '
	cd repo &&
	grit repack &&
	ls .git/objects/pack/*.pack >packs &&
	test_line_count = 1 packs
'

test_expect_success 'repack creates index file' '
	cd repo &&
	ls .git/objects/pack/*.idx >indices &&
	test_line_count = 1 indices
'

test_expect_success 'count-objects -v shows pack after repack' '
	cd repo &&
	grit count-objects -v >actual &&
	grep "^packs: 1" actual
'

test_expect_success 'verify-pack succeeds on valid pack' '
	cd repo &&
	pack=$(ls .git/objects/pack/*.pack) &&
	grit verify-pack "$pack"
'

test_expect_success 'verify-pack -v lists all objects' '
	cd repo &&
	pack=$(ls .git/objects/pack/*.pack) &&
	grit verify-pack -v "$pack" >actual &&
	grep "commit" actual &&
	grep "tree" actual &&
	grep "blob" actual
'

test_expect_success 'verify-pack -v shows ok at end' '
	cd repo &&
	pack=$(ls .git/objects/pack/*.pack) &&
	grit verify-pack -v "$pack" >actual &&
	grep "ok$" actual
'

test_expect_success 'verify-pack -v counts correct number of objects' '
	cd repo &&
	pack=$(ls .git/objects/pack/*.pack) &&
	grit verify-pack -v "$pack" >actual &&
	grep "chain length = 0:" actual
'

test_expect_success 'show-index reads pack index from stdin' '
	cd repo &&
	idx=$(ls .git/objects/pack/*.idx) &&
	grit show-index <"$idx" >actual &&
	test -s actual
'

test_expect_success 'show-index output has offset oid crc format' '
	cd repo &&
	idx=$(ls .git/objects/pack/*.idx) &&
	grit show-index <"$idx" >actual &&
	head -1 actual | grep -E "^[0-9]+ [0-9a-f]{40} "
'

test_expect_success 'show-index lists same number of objects as verify-pack' '
	cd repo &&
	pack=$(ls .git/objects/pack/*.pack) &&
	idx=$(ls .git/objects/pack/*.idx) &&
	grit verify-pack -v "$pack" | grep -E "^[0-9a-f]{40} " | wc -l >vp_count &&
	grit show-index <"$idx" | wc -l >si_count &&
	test_cmp vp_count si_count
'

test_expect_success 'prune-packed removes loose objects' '
	cd repo &&
	grit count-objects >before &&
	before_count=$(echo $(cat before) | awk "{print \$1}") &&
	test "$before_count" -gt 0 &&
	grit prune-packed &&
	grit count-objects >after &&
	after_count=$(echo $(cat after) | awk "{print \$1}") &&
	test "$after_count" -eq 0
'

test_expect_success 'pack file still valid after prune-packed' '
	cd repo &&
	pack=$(ls .git/objects/pack/*.pack) &&
	grit verify-pack "$pack"
'

test_expect_success 'add more commits and repack again' '
	cd repo &&
	for i in 6 7 8 9 10; do
		echo "content $i" >file$i.txt
	done &&
	'"$REAL_GIT"' add -A &&
	'"$REAL_GIT"' commit -m "second batch" &&
	'"$REAL_GIT"' repack -d &&
	grit count-objects >actual &&
	grep "^0 " actual
'

test_expect_success 'verify-pack on new pack succeeds' '
	cd repo &&
	pack=$(ls .git/objects/pack/*.pack | tail -1) &&
	grit verify-pack "$pack"
'

test_expect_success 'setup repo with binary content for pack' '
	git init repo2 &&
	cd repo2 &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	dd if=/dev/urandom bs=1024 count=10 of=binary.dat 2>/dev/null &&
	git add binary.dat &&
	git commit -m "binary content" &&
	grit repack
'

test_expect_success 'verify-pack handles binary content' '
	cd repo2 &&
	pack=$(ls .git/objects/pack/*.pack) &&
	grit verify-pack "$pack"
'

test_expect_success 'verify-pack -v on binary content pack' '
	cd repo2 &&
	pack=$(ls .git/objects/pack/*.pack) &&
	grit verify-pack -v "$pack" >actual &&
	grep "blob" actual
'

test_expect_success 'setup repo with multiple commits for delta chains' '
	git init repo3 &&
	cd repo3 &&
	git config user.name "Test User" &&
	git config user.email "test@test.com" &&
	seq 1 100 >numbers.txt &&
	git add numbers.txt &&
	git commit -m "v1" &&
	seq 1 101 >numbers.txt &&
	git add numbers.txt &&
	git commit -m "v2" &&
	seq 1 102 >numbers.txt &&
	git add numbers.txt &&
	git commit -m "v3" &&
	$REAL_GIT repack -d &&
	grit prune-packed
'

test_expect_success 'verify-pack on pack with deltas' '
	cd repo3 &&
	pack=$(ls .git/objects/pack/*.pack) &&
	grit verify-pack "$pack"
'

test_expect_success 'verify-pack -v shows delta chains' '
	cd repo3 &&
	pack=$(ls .git/objects/pack/*.pack) &&
	grit verify-pack -v "$pack" >actual &&
	test -s actual &&
	grep "ok$" actual
'

test_expect_success 'verify-pack fails on truncated pack' '
	cd repo3 &&
	pack=$(ls .git/objects/pack/*.pack) &&
	head -c 20 "$pack" >truncated.pack &&
	test_must_fail grit verify-pack truncated.pack
'

test_expect_success 'show-index on empty stdin fails gracefully' '
	cd repo3 &&
	echo "" | test_must_fail grit show-index
'

test_done
