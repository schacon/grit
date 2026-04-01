#!/bin/sh
# Ported subset from git/t/t2004-checkout-cache-temp.sh

test_description='grit checkout-index --temp'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup stage-0 entries' '
	grit init repo &&
	cd repo &&
	echo tree1path0 >path0 &&
	echo tree1path1 >path1 &&
	mkdir asubdir &&
	echo tree1asubdir/path5 >asubdir/path5 &&
	grit update-index --add path0 path1 asubdir/path5
'

test_expect_success 'checkout one stage 0 path to temporary file' '
	cd repo &&
	rm -f actual &&
	grit checkout-index --temp -- path1 >actual &&
	test "$(wc -l <actual | tr -d " ")" = "1" &&
	test "$(cut -f2 actual)" = "path1" &&
	p=$(cut -f1 actual) &&
	test_path_is_file "$p" &&
	test "$(cat "$p")" = "tree1path1"
'

test_expect_success 'checkout all stage 0 paths to temporary files' '
	cd repo &&
	rm -f actual &&
	grit checkout-index -a --temp >actual &&
	test "$(wc -l <actual | tr -d " ")" = "3" &&
	for f in path0 path1 asubdir/path5
	do
		test "$(grep "$f" actual | cut -f2)" = "$f" || return 1 &&
		p=$(grep "$f" actual | cut -f1) &&
		test_path_is_file "$p" || return 1
	done
'

test_done
