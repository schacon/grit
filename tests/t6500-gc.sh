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
	gust init repo &&
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

test_done
