#!/bin/sh
# Ported subset from git/t/t5301-sliding-window.sh.

test_description='verify-pack -v basic behavior on generated pack'

. ./test-lib.sh

REAL_GIT=${REAL_GIT:-/usr/bin/git}

test_expect_success 'setup packed repository fixture' '
	grit init repo &&
	cd repo &&
	echo one >one &&
	git update-index --add one &&
	tree=$(git write-tree) &&
	commit1=$(echo commit1 | git commit-tree "$tree") &&
	git update-ref HEAD "$commit1" &&
	"$REAL_GIT" repack -a -d &&
	test "$(git count-objects)" = "0 objects, 0 kilobytes" &&
	pack1=$(echo .git/objects/pack/*.pack) &&
	test_path_is_file "$pack1"
'

test_expect_success 'verify-pack -v accepts .pack path' '
	cd repo &&
	pack1=$(echo .git/objects/pack/*.pack) &&
	git verify-pack -v "$pack1" >out &&
	grep "^$pack1: ok\$" out
'

test_expect_success 'verify-pack -v accepts .idx path' '
	cd repo &&
	pack1=$(echo .git/objects/pack/*.pack) &&
	idx1=${pack1%.pack}.idx &&
	git verify-pack -v "$idx1" >out &&
	grep "^$pack1: ok\$" out
'

test_done
