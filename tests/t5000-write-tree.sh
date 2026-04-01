#!/bin/sh
# Ported from git/t/t0000-basic.sh and git/t/t1020-subdirectory.sh
# (harness-compatible write-tree subset).

test_description='gust write-tree'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'write-tree after update-index --add' '
	gust init repo &&
	cd repo &&
	: >should-be-empty &&
	gust update-index --add should-be-empty &&
	tree=$(gust write-tree) &&
	gust cat-file -p "$tree" >current &&
	cat >expected <<-\EOF &&
	100644 blob e69de29bb2d1d6434b8b29ae775ad8c2e48c5391	should-be-empty
	EOF
	test_cmp expected current
'

test_expect_success 'write-tree can write empty tree' '
	cd repo &&
	rm -f should-be-empty &&
	gust update-index --remove should-be-empty &&
	tree=$(gust write-tree) &&
	gust cat-file -p "$tree" >current &&
	! test -s current
'

test_expect_success 'write-tree --prefix matches subtree oid' '
	cd repo &&
	mkdir -p path3/subp3 &&
	echo "hello path0" >path0 &&
	echo "hello path3/file3" >path3/file3 &&
	echo "hello path3/subp3/file3" >path3/subp3/file3 &&
	gust update-index --add path0 path3/file3 path3/subp3/file3 &&
	tree=$(gust write-tree) &&
	gust ls-tree "$tree" >root-tree &&
	path3_oid=$(awk '\''$4=="path3"{print $3}'\'' root-tree) &&
	ptree=$(gust write-tree --prefix=path3) &&
	test "$ptree" = "$path3_oid" &&
	gust ls-tree "$ptree" >path3-tree &&
	subp3_oid=$(awk '\''$4=="subp3"{print $3}'\'' path3-tree) &&
	pptree=$(gust write-tree --prefix=path3/subp3) &&
	test "$pptree" = "$subp3_oid"
'

test_expect_success 'write-tree fails on missing objects unless --missing-ok' '
	cd repo &&
	rm -f .git/index &&
	cat >badobjects <<-\EOF &&
	100644 blob 1111111111111111111111111111111111111111	dir/file1
	100644 blob 2222222222222222222222222222222222222222	dir/file2
	EOF
	gust update-index --index-info <badobjects &&
	test_must_fail gust write-tree &&
	gust write-tree --missing-ok >/dev/null
'

test_expect_success 'write-tree from subdirectory equals top-level tree' '
	cd repo &&
	rm -f .git/index &&
	echo "one" >one &&
	mkdir -p dir &&
	echo "two" >dir/two &&
	gust update-index --add one dir/two &&
	top=$(gust write-tree) &&
	(
		cd dir &&
		sub=$(gust write-tree) &&
		test "z$top" = "z$sub"
	)
'

test_done
