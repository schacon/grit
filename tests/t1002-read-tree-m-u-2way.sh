#!/bin/sh
# Ported subset from git/t/t1002-read-tree-m-u-2way.sh.

test_description='gust read-tree -m -u two-way updates worktree'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup H and M trees and file snapshots' '
	gust init repo &&
	cd repo &&
	echo bozbar >bozbar &&
	echo nitfol >nitfol &&
	echo rezrov >rezrov &&
	rm -f .git/index &&
	gust update-index --add bozbar nitfol rezrov &&
	tree_h=$(gust write-tree) &&
	echo "$tree_h" >../tree_h &&
	echo gnusto >bozbar &&
	echo frotz >frotz &&
	rm -f .git/index &&
	gust update-index --add bozbar frotz nitfol &&
	tree_m=$(gust write-tree) &&
	echo "$tree_m" >../tree_m &&
	cp bozbar bozbar.M &&
	cp frotz frotz.M &&
	cp nitfol nitfol.M
'

test_expect_success 'read-tree -m -u writes merged result into worktree' '
	cd repo &&
	rm -f .git/index bozbar nitfol rezrov frotz &&
	gust read-tree --reset -u "$(cat ../tree_h)" &&
	gust read-tree -m -u "$(cat ../tree_h)" "$(cat ../tree_m)" &&
	test_cmp bozbar.M bozbar &&
	test_cmp frotz.M frotz &&
	test_cmp nitfol.M nitfol &&
	test_path_is_missing rezrov
'

test_expect_success 'read-tree -m -u fails on conflicting local change' '
	cd repo &&
	gust read-tree --reset -u "$(cat ../tree_h)" &&
	echo local-change >bozbar &&
	gust update-index --add bozbar &&
	test_must_fail gust read-tree -m -u "$(cat ../tree_h)" "$(cat ../tree_m)"
'

test_done
