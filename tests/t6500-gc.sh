#!/bin/sh
# Ported subset from git/t/t6500-gc.sh.

test_description='gc basic packing, auto gating, and prune behavior'

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
# Basic gc tests
# ---------------------------------------------------------------------------

test_expect_success 'setup repository' '
	grit init repo &&
	cd repo
'

test_expect_success 'gc packs loose objects by default' '
	cd repo &&
	create_commit reachable reachable.txt reachable &&
	loose=$(git count-objects | sed "s/ .*//") &&
	test "$loose" -gt 0 &&
	git gc &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes" &&
	pack=$(echo .git/objects/pack/*.pack) &&
	test_path_is_file "$pack"
'

test_expect_success 'gc --auto honors gc.auto=0' '
	cd repo &&
	"$REAL_GIT" config gc.auto 0 &&
	echo auto >auto.txt &&
	git hash-object -w auto.txt >/dev/null &&
	before=$(git count-objects | sed "s/ .*//") &&
	git gc --auto &&
	after=$(git count-objects | sed "s/ .*//") &&
	test "$before" = "$after"
'

test_expect_success 'gc --prune=now removes unreachable loose object' '
	cd repo &&
	echo unreachable >unreachable.txt &&
	oid=$(git hash-object -w unreachable.txt) &&
	loose=.git/objects/$(echo "$oid" | sed "s/^../&\//") &&
	test_path_is_file "$loose" &&
	git gc --prune=now &&
	test_path_is_missing "$loose"
'

# ---------------------------------------------------------------------------
# gc on empty and simple repos
# ---------------------------------------------------------------------------

test_expect_success 'gc empty repository' '
	rm -rf repo_gc_empty &&
	grit init repo_gc_empty &&
	cd repo_gc_empty &&
	git gc
'

test_expect_success 'gc does not leave behind pid file' '
	cd repo_gc_empty &&
	git gc &&
	test_path_is_missing .git/gc.pid
'

test_expect_success 'gc --quiet produces no output' '
	rm -rf repo_gcq &&
	grit init repo_gcq &&
	cd repo_gcq &&
	create_commit base one.txt one &&
	git gc --quiet >stdout 2>stderr &&
	test_must_be_empty stdout &&
	test_must_be_empty stderr
'

test_expect_success 'gc after repack -a -d still works' '
	rm -rf repo_gc_after &&
	grit init repo_gc_after &&
	cd repo_gc_after &&
	create_commit base one.txt one &&
	git repack -a -d &&
	git gc &&
	test_path_is_file "$(echo .git/objects/pack/*.pack)"
'

test_expect_success 'gc is idempotent' '
	rm -rf repo_gc_idem &&
	grit init repo_gc_idem &&
	cd repo_gc_idem &&
	create_commit base one.txt one &&
	git gc &&
	git gc &&
	test_path_is_file "$(echo .git/objects/pack/*.pack)"
'

# ---------------------------------------------------------------------------
# Additional gc tests ported from t6500
# ---------------------------------------------------------------------------

test_expect_success 'gc creates a pack file' '
	rm -rf repo_gc_pack &&
	grit init repo_gc_pack &&
	cd repo_gc_pack &&
	create_commit first one.txt one &&
	git gc &&
	pack_count=$(ls .git/objects/pack/*.pack 2>/dev/null | wc -l) &&
	test "$pack_count" -ge 1
'

test_expect_success 'gc removes loose objects' '
	rm -rf repo_gc_loose &&
	grit init repo_gc_loose &&
	cd repo_gc_loose &&
	create_commit first one.txt one &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	test "$loose_before" -gt 0 &&
	git gc &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes"
'

test_expect_success 'gc --prune=now with only packed objects is safe' '
	rm -rf repo_gc_prune_safe &&
	grit init repo_gc_prune_safe &&
	cd repo_gc_prune_safe &&
	create_commit first one.txt one &&
	git gc &&
	git gc --prune=now &&
	pack=$(echo .git/objects/pack/*.pack) &&
	test_path_is_file "$pack" &&
	git verify-pack "$pack"
'

test_expect_success 'gc produces valid pack verified by verify-pack' '
	rm -rf repo_gc_vp &&
	grit init repo_gc_vp &&
	cd repo_gc_vp &&
	create_commit first one.txt one &&
	create_commit second two.txt two &&
	git gc &&
	for p in .git/objects/pack/*.pack; do
		git verify-pack "$p" || return 1
	done
'

test_expect_success 'gc with multiple commits produces correct pack' '
	rm -rf repo_gc_mc &&
	grit init repo_gc_mc &&
	cd repo_gc_mc &&
	create_commit first one.txt one &&
	create_commit second two.txt two &&
	create_commit third three.txt three &&
	git gc &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	commit_count=$(grep -c " commit " out) &&
	test "$commit_count" = "3"
'

test_expect_success 'gc --auto with gc.auto=0 is no-op' '
	rm -rf repo_gc_auto0 &&
	grit init repo_gc_auto0 &&
	cd repo_gc_auto0 &&
	"$REAL_GIT" config gc.auto 0 &&
	create_commit base one.txt one &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	git gc --auto &&
	loose_after=$(git count-objects | sed "s/ .*//") &&
	test "$loose_before" = "$loose_after"
'

test_expect_success 'gc packs blobs trees and commits' '
	rm -rf repo_gc_types &&
	grit init repo_gc_types &&
	cd repo_gc_types &&
	create_commit first one.txt one &&
	git gc &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack" >out &&
	grep " blob " out &&
	grep " tree " out &&
	grep " commit " out
'

test_expect_success 'gc with tag objects' '
	rm -rf repo_gc_tag &&
	grit init repo_gc_tag &&
	cd repo_gc_tag &&
	create_commit first one.txt one &&
	git tag v1.0 HEAD &&
	git gc &&
	pack=$(echo .git/objects/pack/*.pack) &&
	git verify-pack "$pack"
'

test_expect_success 'count-objects -v reports correct state after gc' '
	rm -rf repo_gc_co &&
	grit init repo_gc_co &&
	cd repo_gc_co &&
	create_commit first one.txt one &&
	git gc &&
	git count-objects -v >out &&
	grep "^count: 0\$" out &&
	in_pack=$(grep "^in-pack:" out | sed "s/^in-pack: //") &&
	test "$in_pack" -ge 3 &&
	grep "^packs: 1\$" out
'

test_expect_success 'gc followed by prune-packed is a no-op on loose objects' '
	rm -rf repo_gc_pp &&
	grit init repo_gc_pp &&
	cd repo_gc_pp &&
	create_commit first one.txt one &&
	git gc &&
	test "$(git count-objects | sed "s/ .*//")" = "0" &&
	grit prune-packed &&
	test "$(git count-objects | sed "s/ .*//")" = "0"
'

test_expect_success 'gc works with repo that has only blobs' '
	rm -rf repo_gc_blob &&
	grit init repo_gc_blob &&
	cd repo_gc_blob &&
	echo a | git hash-object -w --stdin >/dev/null &&
	echo b | git hash-object -w --stdin >/dev/null &&
	echo c | git hash-object -w --stdin >/dev/null &&
	loose=$(git count-objects | sed "s/ .*//") &&
	test "$loose" = "3" &&
	git gc --prune=now &&
	test "$(git count-objects | sed "s/ .*//")" = "0"
'

test_done
