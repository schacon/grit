#!/bin/sh
# Ported subset from git/t/t7700-repack.sh.

test_description='repack basic modes and alternates interaction'

. ./test-lib.sh

REAL_GIT=${REAL_GIT:-/usr/bin/git}

create_commit () {
	msg=$1 &&
	file=$2 &&
	content=$3 &&
	parent_arg= &&
	echo "$content" >"$file" &&
	git update-index --add "$file" &&
	tree=$(git write-tree) &&
	if head_oid=$(git rev-parse --verify HEAD 2>/dev/null)
	then
		parent_arg="-p $head_oid"
	fi &&
	commit=$(echo "$msg" | git commit-tree "$tree" $parent_arg) &&
	git update-ref HEAD "$commit"
}

# ---------------------------------------------------------------------------
# Basic setup
# ---------------------------------------------------------------------------

test_expect_success 'setup repository with loose objects' '
	grit init repo &&
	cd repo &&
	create_commit base one.txt one &&
	loose=$(git count-objects | sed "s/ .*//") &&
	test "$loose" -gt 0
'

test_expect_success 'repack -a -d packs loose objects' '
	cd repo &&
	git repack -a -d &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes" &&
	packs=$(echo .git/objects/pack/*.pack) &&
	test_path_is_file "$packs"
'

test_expect_success 'repack accepts pack-objects tuning flags' '
	cd repo &&
	echo three >three.txt &&
	git hash-object -w three.txt >/dev/null &&
	git repack -a -d -l -f -F --window=5 --depth=20 &&
	pack=$(echo .git/objects/pack/*.pack) &&
	test_path_is_file "$pack"
'

test_expect_success 'loose objects in alternate ODB are not repacked' '
	cd repo &&
	mkdir -p alt_objects &&
	echo "$(pwd)/alt_objects" >.git/objects/info/alternates &&
	alt_oid=$(GIT_OBJECT_DIRECTORY=alt_objects "$REAL_GIT" hash-object -w --stdin <<-\EOF
	from alternate
	EOF
	) &&
	git repack -a -d -l &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git verify-pack -v "$idx" >packlist &&
	! grep "^$alt_oid " packlist
'

# ---------------------------------------------------------------------------
# repack with no objects
# ---------------------------------------------------------------------------

test_expect_success 'repack with no objects is a no-op' '
	rm -rf repo_empty &&
	grit init repo_empty &&
	cd repo_empty &&
	git repack &&
	test -z "$(ls .git/objects/pack/*.pack 2>/dev/null)"
'

# ---------------------------------------------------------------------------
# repack -a
# ---------------------------------------------------------------------------

test_expect_success 'repack -a creates pack from loose objects' '
	rm -rf repo_ra &&
	grit init repo_ra &&
	cd repo_ra &&
	create_commit base one.txt one &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	test "$loose_before" -gt 0 &&
	git repack -a &&
	packs=$(echo .git/objects/pack/*.pack) &&
	test_path_is_file "$packs"
'

test_expect_success 'repack -a -d removes loose objects' '
	rm -rf repo_rad &&
	grit init repo_rad &&
	cd repo_rad &&
	create_commit base one.txt one &&
	git repack -a -d &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes" &&
	test_path_is_file "$(echo .git/objects/pack/*.pack)"
'

test_expect_success 'repack -l only packs local objects' '
	rm -rf repo_rl &&
	grit init repo_rl &&
	cd repo_rl &&
	create_commit base one.txt one &&
	git repack -a -d -l &&
	test_path_is_file "$(echo .git/objects/pack/*.pack)"
'

test_expect_success 'incremental repack creates additional pack' '
	rm -rf repo_inc &&
	grit init repo_inc &&
	cd repo_inc &&
	create_commit first one.txt one &&
	git repack -a -d &&
	pack_count_before=$(ls .git/objects/pack/*.pack 2>/dev/null | wc -l) &&
	echo new_content >two.txt &&
	git hash-object -w two.txt >/dev/null &&
	loose_after=$(git count-objects | sed "s/ .*//") &&
	test "$loose_after" -gt 0 &&
	git repack &&
	pack_count_after=$(ls .git/objects/pack/*.pack 2>/dev/null | wc -l) &&
	test "$pack_count_after" -ge "$pack_count_before"
'

test_expect_success 'repack with -f -F flags' '
	rm -rf repo_ff &&
	grit init repo_ff &&
	cd repo_ff &&
	create_commit base one.txt one &&
	git repack -a -d -f -F &&
	test_path_is_file "$(echo .git/objects/pack/*.pack)"
'

test_expect_success 'repack --window and --depth options' '
	rm -rf repo_wd &&
	grit init repo_wd &&
	cd repo_wd &&
	create_commit base one.txt one &&
	git repack -a -d --window=5 --depth=20 &&
	test_path_is_file "$(echo .git/objects/pack/*.pack)"
'

test_expect_success 'repack preserves objects reachable from HEAD' '
	rm -rf repo_reach &&
	grit init repo_reach &&
	cd repo_reach &&
	create_commit first one.txt one &&
	create_commit second two.txt two &&
	git repack -a &&
	git cat-file -t HEAD &&
	parent=$(git rev-parse HEAD~1) &&
	git cat-file -t $parent
'

# ---------------------------------------------------------------------------
# Additional repack tests ported from t7700
# ---------------------------------------------------------------------------

test_expect_success 'repack -a -d produces single pack' '
	rm -rf repo_single &&
	grit init repo_single &&
	cd repo_single &&
	create_commit first one.txt one &&
	create_commit second two.txt two &&
	git repack -a -d &&
	pack_count=$(ls .git/objects/pack/*.pack | wc -l) &&
	test "$pack_count" = "1"
'

test_expect_success 'repeated repack -a -d is idempotent' '
	cd repo_single &&
	pack_before=$(ls .git/objects/pack/*.pack) &&
	git repack -a -d &&
	git repack -a -d &&
	pack_after=$(ls .git/objects/pack/*.pack) &&
	test_path_is_file "$pack_after"
'

test_expect_success 'repack -q suppresses progress output' '
	rm -rf repo_quiet &&
	grit init repo_quiet &&
	cd repo_quiet &&
	create_commit base one.txt one &&
	git repack -a -d -q >stdout 2>stderr &&
	test_must_be_empty stdout
'

test_expect_success 'repack after adding new objects creates new pack' '
	rm -rf repo_newobj &&
	grit init repo_newobj &&
	cd repo_newobj &&
	create_commit first one.txt one &&
	git repack -a &&
	pack1=$(echo .git/objects/pack/*.pack) &&
	create_commit second two.txt two &&
	git repack -a &&
	pack2_count=$(ls .git/objects/pack/*.pack | wc -l) &&
	test "$pack2_count" -ge 1
'

test_expect_success 'verify-pack passes after repack -a' '
	cd repo_newobj &&
	for p in .git/objects/pack/*.pack; do
		git verify-pack "$p" || return 1
	done
'

test_expect_success 'repack -a -d with many objects' '
	rm -rf repo_many &&
	grit init repo_many &&
	cd repo_many &&
	i=1 &&
	while test $i -le 30; do
		echo "content $i" >file_$i.txt &&
		i=$(($i + 1))
	done &&
	git update-index --add file_*.txt &&
	tree=$(git write-tree) &&
	commit=$(echo "many files" | git commit-tree "$tree") &&
	git update-ref HEAD "$commit" &&
	git repack -a -d &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes" &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	obj_count=$(grep -cE "^[0-9a-f]{40}" out) &&
	test "$obj_count" -ge 32
'

test_expect_success 'repack preserves multiple commits in chain (verify-pack)' '
	rm -rf repo_chain &&
	grit init repo_chain &&
	cd repo_chain &&
	create_commit first one.txt one &&
	create_commit second two.txt two &&
	create_commit third three.txt three &&
	git repack -a -d &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	commit_count=$(grep -c " commit " out) &&
	test "$commit_count" = "3"
'

test_expect_success 'alternates: repack does not pack alternate loose objects with -l' '
	rm -rf repo_alt &&
	grit init repo_alt &&
	cd repo_alt &&
	mkdir -p alt_odb &&
	echo "$(pwd)/alt_odb" >.git/objects/info/alternates &&
	alt_blob=$(echo "alt content" | GIT_OBJECT_DIRECTORY=alt_odb "$REAL_GIT" hash-object -w --stdin) &&
	create_commit local local.txt local &&
	git repack -a -d -l &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git verify-pack -v "$idx" >packed &&
	! grep "^$alt_blob " packed
'

test_expect_success 'repack removes old packs when using -d' '
	rm -rf repo_old_packs &&
	grit init repo_old_packs &&
	cd repo_old_packs &&
	create_commit first one.txt one &&
	git repack &&
	pack_count_before=$(ls .git/objects/pack/*.pack | wc -l) &&
	test "$pack_count_before" -ge 1 &&
	create_commit second two.txt two &&
	git repack &&
	pack_count_mid=$(ls .git/objects/pack/*.pack | wc -l) &&
	test "$pack_count_mid" -ge 2 &&
	git repack -a -d &&
	pack_count_after=$(ls .git/objects/pack/*.pack | wc -l) &&
	test "$pack_count_after" = "1"
'

test_expect_success 'repack creates valid index' '
	cd repo_old_packs &&
	idx=$(echo .git/objects/pack/*.idx) &&
	test_path_is_file "$idx" &&
	git verify-pack "$idx"
'

test_expect_success 'repack -a packs objects from all existing packs' '
	rm -rf repo_multipack &&
	grit init repo_multipack &&
	cd repo_multipack &&
	create_commit first one.txt one &&
	git repack &&
	create_commit second two.txt two &&
	git repack &&
	pack_count=$(ls .git/objects/pack/*.pack | wc -l) &&
	test "$pack_count" -ge 2 &&
	git repack -a -d &&
	pack_count_after=$(ls .git/objects/pack/*.pack | wc -l) &&
	test "$pack_count_after" = "1" &&
	git verify-pack -v .git/objects/pack/*.pack >out &&
	grep "commit" out
'

test_expect_success 'count-objects shows 0 after repack -a -d' '
	cd repo_multipack &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes"
'

test_expect_success 'repack packs blob objects' '
	rm -rf repo_catfile &&
	grit init repo_catfile &&
	cd repo_catfile &&
	create_commit first one.txt one &&
	blob_oid=$(git hash-object one.txt) &&
	git repack -a -d &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	grep "^$blob_oid " out
'

test_expect_success 'repack does not lose tagged objects' '
	rm -rf repo_tags &&
	grit init repo_tags &&
	cd repo_tags &&
	create_commit first one.txt one &&
	git tag v1.0 HEAD &&
	tag_oid=$(git rev-parse v1.0) &&
	git repack -a -d &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	grep "^$tag_oid " out
'

test_done
