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

test_done
