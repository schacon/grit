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
# Additional repack tests
# ---------------------------------------------------------------------------

test_expect_success 'repack with no objects is a no-op' '
	rm -rf repo_empty &&
	grit init repo_empty &&
	cd repo_empty &&
	git repack &&
	test -z "$(ls .git/objects/pack/*.pack 2>/dev/null)"
'

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

test_done
