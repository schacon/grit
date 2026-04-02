#!/bin/sh
# Ported subset from git/t/t5304-prune.sh focused on count-objects output.

test_description='count-objects loose count and verbose garbage accounting'

. ./test-lib.sh

test_expect_success 'count-objects loose count changes with hash-object -w' '
	grit init repo &&
	cd repo &&
	before=$(git count-objects | sed "s/ .*//") &&
	BLOB=$(echo aleph_0 | git hash-object -w --stdin) &&
	BLOB_FILE=.git/objects/$(echo "$BLOB" | sed "s/^../&\//") &&
	after=$(git count-objects | sed "s/ .*//") &&
	test $((before + 1)) = "$after" &&
	test_path_is_file "$BLOB_FILE"
'

test_expect_success 'count-objects -v reports garbage files' '
	cd repo &&
	mkdir -p .git/objects/pack &&
	>.git/objects/pack/fake.bar &&
	git count-objects -v >actual &&
	grep "^garbage: 1\$" actual
'

# ---------------------------------------------------------------------------
# Additional count-objects tests
# ---------------------------------------------------------------------------

test_expect_success 'count-objects with zero loose objects' '
	rm -rf repo_co0 &&
	grit init repo_co0 &&
	cd repo_co0 &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes"
'

test_expect_success 'count-objects shows increasing count' '
	rm -rf repo_co1 &&
	grit init repo_co1 &&
	cd repo_co1 &&
	before=$(git count-objects | sed "s/ .*//") &&
	echo blob1 | git hash-object -w --stdin >/dev/null &&
	after1=$(git count-objects | sed "s/ .*//") &&
	test $((before + 1)) = "$after1" &&
	echo blob2 | git hash-object -w --stdin >/dev/null &&
	after2=$(git count-objects | sed "s/ .*//") &&
	test $((before + 2)) = "$after2"
'

test_expect_success 'count-objects -v shows verbose output' '
	cd repo_co1 &&
	git count-objects -v >out &&
	grep "^count:" out &&
	grep "^size:" out &&
	grep "^in-pack:" out &&
	grep "^packs:" out
'

test_expect_success 'count-objects -v reports multiple garbage files' '
	rm -rf repo_co_garb &&
	grit init repo_co_garb &&
	cd repo_co_garb &&
	mkdir -p .git/objects/pack &&
	>.git/objects/pack/fake1.bar &&
	>.git/objects/pack/fake2.baz &&
	git count-objects -v >actual &&
	grep "^garbage: 2\$" actual &&
	rm .git/objects/pack/fake1.bar .git/objects/pack/fake2.baz
'

test_expect_success 'count-objects -v size-pack updates after repack' '
	rm -rf repo_cosp &&
	grit init repo_cosp &&
	cd repo_cosp &&
	echo content | git hash-object -w --stdin >/dev/null &&
	git count-objects -v >before_repack &&
	grep "^size-pack: 0" before_repack &&
	echo content2 >f.txt &&
	git add f.txt &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	git commit -m init &&
	git repack -a -d &&
	git count-objects -v >after_repack &&
	grep "^size-pack:" after_repack | grep -v "size-pack: 0"
'

test_expect_success 'count-objects returns 0 after full repack -a -d' '
	rm -rf repo_coall &&
	grit init repo_coall &&
	cd repo_coall &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo content >f.txt &&
	git add f.txt &&
	git commit -m init &&
	git repack -a -d &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes"
'

test_expect_success 'count-objects -v in-pack count matches verify-pack' '
	cd repo_coall &&
	git count-objects -v >co_out &&
	in_pack=$(grep "^in-pack:" co_out | sed "s/^in-pack: //") &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git verify-pack -v "$idx" >vp_out &&
	obj_count=$(grep -c -E "^[0-9a-f]{40}" vp_out) &&
	test "$in_pack" = "$obj_count"
'

test_expect_success 'count-objects output format matches expected pattern' '
	rm -rf repo_fmt &&
	grit init repo_fmt &&
	cd repo_fmt &&
	out=$(git count-objects) &&
	echo "$out" | grep -E "^[0-9]+ objects, [0-9]+ kilobytes$"
'

test_expect_success 'count-objects -v prune-packable is zero after prune-packed' '
	rm -rf repo_pp &&
	grit init repo_pp &&
	cd repo_pp &&
	echo blob | git hash-object -w --stdin >/dev/null &&
	git repack -a -d &&
	git prune-packed &&
	git count-objects -v >out &&
	grep "^prune-packable: 0$" out
'

test_expect_success 'count-objects -v size is positive after adding large object' '
	rm -rf repo_sz &&
	grit init repo_sz &&
	cd repo_sz &&
	dd if=/dev/urandom bs=1024 count=4 2>/dev/null | git hash-object -w --stdin >/dev/null &&
	git count-objects -v >out &&
	size=$(grep "^size:" out | sed "s/^size: //") &&
	test "$size" -gt 0
'

test_expect_success 'count-objects in-pack increases after repack' '
	rm -rf repo_inpack &&
	grit init repo_inpack &&
	cd repo_inpack &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	echo content >f.txt &&
	git add f.txt &&
	git commit -m init &&
	git count-objects -v >before &&
	inpack_before=$(grep "^in-pack:" before | sed "s/^in-pack: //") &&
	test "$inpack_before" -eq 0 &&
	git repack -a -d &&
	git count-objects -v >after &&
	inpack_after=$(grep "^in-pack:" after | sed "s/^in-pack: //") &&
	test "$inpack_after" -gt 0
'

test_expect_success 'count-objects loose goes to zero after repack -a -d' '
	rm -rf repo_cozero &&
	grit init repo_cozero &&
	cd repo_cozero &&
	git config user.email "t@t.com" &&
	git config user.name "T" &&
	echo content >f.txt &&
	git add f.txt &&
	git commit -m init &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	test "$loose_before" -gt 0 &&
	git repack -a -d &&
	loose_after=$(git count-objects | sed "s/ .*//") &&
	test "$loose_after" -eq 0
'

test_done
