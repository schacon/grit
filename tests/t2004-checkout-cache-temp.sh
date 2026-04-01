#!/bin/sh
# Ported subset from git/t/t2004-checkout-cache-temp.sh

test_description='grit checkout-index --temp'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	mkdir asubdir &&
	echo tree1path0 >path0 &&
	echo tree1path1 >path1 &&
	echo tree1path3 >path3 &&
	echo tree1path4 >path4 &&
	echo tree1asubdir/path5 >asubdir/path5 &&
	git update-index --add path0 path1 path3 path4 asubdir/path5 &&
	git write-tree >../t1 &&
	rm -f path* .merge_* actual .git/index &&
	echo tree2path0 >path0 &&
	echo tree2path1 >path1 &&
	echo tree2path2 >path2 &&
	echo tree2path4 >path4 &&
	git update-index --add path0 path1 path2 path4 &&
	git write-tree >../t2 &&
	rm -f path* .merge_* actual .git/index &&
	echo tree2path0 >path0 &&
	echo tree3path1 >path1 &&
	echo tree3path2 >path2 &&
	echo tree3path3 >path3 &&
	git update-index --add path0 path1 path2 path3 &&
	git write-tree >../t3
'

test_expect_success 'checkout one stage 0 to temporary file' '
	cd repo &&
	t1=$(cat ../t1) &&
	rm -f path* .merge_* actual .git/index &&
	git read-tree $t1 &&
	git checkout-index --temp -- path1 >actual &&
	test "$(wc -l <actual | tr -d " ")" = "1" &&
	test "$(cut -f2 actual)" = "path1" &&
	p=$(cut -f1 actual) &&
	test_path_is_file "$p" &&
	test "$(cat "$p")" = "tree1path1"
'

test_expect_success 'checkout all stage 0 to temporary files' '
	cd repo &&
	t1=$(cat ../t1) &&
	rm -f path* .merge_* actual .git/index &&
	git read-tree $t1 &&
	git checkout-index -a --temp >actual &&
	test "$(wc -l <actual | tr -d " ")" = "5" &&
	for f in path0 path1 path3 path4 asubdir/path5
	do
		test "$(grep "$f" actual | cut -f2)" = "$f" &&
		p=$(grep "$f" actual | cut -f1) &&
		test_path_is_file "$p" || return 1
	done
'

test_expect_success 'setup 3-way merge' '
	cd repo &&
	t1=$(cat ../t1) &&
	t2=$(cat ../t2) &&
	t3=$(cat ../t3) &&
	rm -f path* .merge_* actual .git/index &&
	git read-tree -m $t1 $t2 $t3
'

test_expect_success 'checkout one stage 2 to temporary file' '
	cd repo &&
	rm -f path* .merge_* actual &&
	git checkout-index --stage=2 --temp -- path1 >actual &&
	test "$(wc -l <actual | tr -d " ")" = "1" &&
	test "$(cut -f2 actual)" = "path1" &&
	p=$(cut -f1 actual) &&
	test_path_is_file "$p" &&
	test "$(cat "$p")" = "tree2path1"
'

test_expect_success 'checkout all stage 2 to temporary files' '
	cd repo &&
	rm -f path* .merge_* actual &&
	git checkout-index --all --stage=2 --temp >actual &&
	test "$(wc -l <actual | tr -d " ")" = "3" &&
	for f in path1 path2 path4
	do
		test "$(grep "$f" actual | cut -f2)" = "$f" &&
		p=$(grep "$f" actual | cut -f1) &&
		test_path_is_file "$p" || return 1
	done
'

test_expect_success 'checkout all stages of unknown path' '
	cd repo &&
	rm -f path* .merge_* actual &&
	test_must_fail git checkout-index --stage=2 --temp \
		-- does-not-exist 2>stderr &&
	grep "not in" stderr
'

test_done
