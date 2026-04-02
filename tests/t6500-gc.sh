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
# Additional gc tests
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

test_expect_success 'gc --gobbledegook prints usage' '
	rm -rf repo_gunk &&
	grit init repo_gunk &&
	cd repo_gunk &&
	test_must_fail git gc --no-such-option 2>err &&
	grep -i "usage" err
'

test_expect_success 'gc --no-prune flag is accepted without error' '
	rm -rf repo_noprune &&
	grit init repo_noprune &&
	cd repo_noprune &&
	create_commit base one.txt one &&
	git gc --no-prune
'

test_expect_success 'gc packs objects into a pack file' '
	rm -rf repo_gcacc &&
	grit init repo_gcacc &&
	cd repo_gcacc &&
	create_commit base one.txt one &&
	head_oid=$(git rev-parse HEAD) &&
	git gc &&
	idx=$(echo .git/objects/pack/*.idx) &&
	git verify-pack -v "$idx" >packlist &&
	grep "^$head_oid " packlist
'

test_expect_success 'gc --prune=never flag is accepted without error' '
	rm -rf repo_gcnever &&
	grit init repo_gcnever &&
	cd repo_gcnever &&
	create_commit base one.txt one &&
	git gc --prune=never
'

test_expect_success 'gc consolidates multiple packs into one' '
	rm -rf repo_gccons &&
	grit init repo_gccons &&
	cd repo_gccons &&
	create_commit first one.txt one &&
	git repack &&
	create_commit second two.txt two &&
	git repack &&
	packs_before=$(ls .git/objects/pack/*.pack 2>/dev/null | wc -l) &&
	test "$packs_before" -ge 2 &&
	git gc &&
	packs_after=$(ls .git/objects/pack/*.pack | wc -l) &&
	test "$packs_after" -eq 1
'

test_expect_success 'gc explicit run with gc.auto=0 still packs loose objects' '
	rm -rf repo_gcexpl &&
	grit init repo_gcexpl &&
	cd repo_gcexpl &&
	create_commit base one.txt one &&
	git config gc.auto 0 &&
	loose_before=$(git count-objects | sed "s/ .*//") &&
	test "$loose_before" -gt 0 &&
	git gc &&
	test_path_is_file "$(echo .git/objects/pack/*.pack)"
'

test_expect_success 'gc does not leave .tmp files in pack directory' '
	rm -rf repo_gctmp &&
	grit init repo_gctmp &&
	cd repo_gctmp &&
	create_commit base one.txt one &&
	git gc &&
	tmp_count=$(find .git/objects/pack -name ".tmp-*" 2>/dev/null | wc -l) &&
	test "$tmp_count" -eq 0
'

test_done
