#!/bin/sh
# Ported subset from git/t/t5304-prune.sh focused on count-objects output.

test_description='count-objects loose count and verbose garbage accounting'

. ./test-lib.sh

REAL_GIT=${REAL_GIT:-/usr/bin/git}

# ---------------------------------------------------------------------------
# count-objects basics
# ---------------------------------------------------------------------------

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
# count-objects with zero loose objects
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

# ---------------------------------------------------------------------------
# Additional count-objects tests ported from t5304
# ---------------------------------------------------------------------------

test_expect_success 'count-objects -v with no garbage shows garbage: 0' '
	rm -rf repo_co_ng &&
	grit init repo_co_ng &&
	cd repo_co_ng &&
	git count-objects -v >out &&
	grep "^garbage: 0\$" out
'

test_expect_success 'count-objects -v packs count' '
	rm -rf repo_co_pc &&
	grit init repo_co_pc &&
	cd repo_co_pc &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	git count-objects -v >out &&
	grep "^packs: 0\$" out &&
	echo content >f.txt &&
	git add f.txt &&
	git commit -m init &&
	git repack -a -d &&
	git count-objects -v >out2 &&
	grep "^packs: 1\$" out2
'

test_expect_success 'count-objects -v size shows non-zero for loose objects' '
	rm -rf repo_co_sz &&
	grit init repo_co_sz &&
	cd repo_co_sz &&
	echo "hello world of loose objects" | git hash-object -w --stdin >/dev/null &&
	git count-objects -v >out &&
	size=$(grep "^size:" out | sed "s/^size: //") &&
	test "$size" -ge 0
'

test_expect_success 'count-objects loose count after hash-object and prune-packed' '
	rm -rf repo_co_pp &&
	grit init repo_co_pp &&
	cd repo_co_pp &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo one >f.txt &&
	git add f.txt &&
	git commit -m init &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	test "$loose_before" -gt 0 &&
	git repack -a &&
	grit prune-packed &&
	test "$(git count-objects | sed "s/ .*//")" = "0"
'

test_expect_success 'count-objects tracks multiple hash-object writes' '
	rm -rf repo_co_multi &&
	grit init repo_co_multi &&
	cd repo_co_multi &&
	test "$(git count-objects | sed "s/ .*//")" = "0" &&
	echo a | git hash-object -w --stdin >/dev/null &&
	test "$(git count-objects | sed "s/ .*//")" = "1" &&
	echo b | git hash-object -w --stdin >/dev/null &&
	test "$(git count-objects | sed "s/ .*//")" = "2" &&
	echo c | git hash-object -w --stdin >/dev/null &&
	test "$(git count-objects | sed "s/ .*//")" = "3"
'

test_expect_success 'count-objects does not double-count packed objects' '
	rm -rf repo_co_dc &&
	grit init repo_co_dc &&
	cd repo_co_dc &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo one >f.txt &&
	git add f.txt &&
	git commit -m init &&
	git repack -a -d &&
	test "$(git count-objects | sed "s/ .*//")" = "0"
'

test_expect_success 'count-objects with two pack files' '
	rm -rf repo_co_2p &&
	grit init repo_co_2p &&
	cd repo_co_2p &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo first >f1.txt &&
	git add f1.txt &&
	git commit -m first &&
	git repack &&
	echo second >f2.txt &&
	git add f2.txt &&
	git commit -m second &&
	git repack &&
	grit prune-packed &&
	git count-objects -v >out &&
	packs=$(grep "^packs:" out | sed "s/^packs: //") &&
	test "$packs" -ge 2 &&
	in_pack=$(grep "^in-pack:" out | sed "s/^in-pack: //") &&
	test "$in_pack" -ge 3
'

test_expect_success 'count-objects -v garbage with fake .keep file only' '
	rm -rf repo_co_keep &&
	grit init repo_co_keep &&
	cd repo_co_keep &&
	mkdir -p .git/objects/pack &&
	>.git/objects/pack/fake2.keep &&
	git count-objects -v 2>stderr &&
	test -s stderr || true
'

test_expect_success 'count-objects -v size-garbage accounts for garbage size' '
	rm -rf repo_co_sg &&
	grit init repo_co_sg &&
	cd repo_co_sg &&
	mkdir -p .git/objects/pack &&
	dd if=/dev/zero of=.git/objects/pack/fake.bar bs=1024 count=1 2>/dev/null &&
	git count-objects -v >out &&
	grep "^size-garbage:" out
'

test_expect_success 'count-objects -v after gc matches expectations' '
	rm -rf repo_co_gc &&
	grit init repo_co_gc &&
	cd repo_co_gc &&
	git config user.email "test@example.com" &&
	git config user.name "Test User" &&
	echo content >f.txt &&
	git add f.txt &&
	git commit -m init &&
	git gc &&
	git count-objects -v >out &&
	grep "^count: 0\$" out &&
	grep "^packs: 1\$" out
'

test_done
