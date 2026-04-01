#!/bin/sh
# Ported subset from git/t/t1020-subdirectory.sh.
#
# This harness-compatible subset focuses on core plumbing behavior in
# subdirectories and intentionally omits sections that require helpers or
# commands not implemented in this test harness (e.g. lib-read-tree.sh
# wrappers, alias expansion, and bare-repo ambiguity checks).

test_description='grit plumbing commands from subdirectories'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup fixture files' '
	grit init repo &&
	cd repo &&
	long="a b c d e f g h i j k l m n o p q r s t u v w x y z" &&
	echo "$long" | tr " " "\n" >one &&
	mkdir dir &&
	{
		echo x &&
		echo y &&
		echo z &&
		echo "$long" | tr " " "\n" &&
		echo a &&
		echo b &&
		echo c
	} >dir/two &&
	cp one original.one &&
	cp dir/two original.two
'

test_expect_success 'update-index and ls-files from subdirectory' '
	cd repo &&
	grit update-index --add one &&
	cat >expect <<-\EOF &&
	one
	EOF
	grit ls-files >actual &&
	test_cmp expect actual &&
	(
		cd dir &&
		grit update-index --add two &&
		cat >expect <<-\EOF &&
		two
		EOF
		grit ls-files >actual &&
		test_cmp expect actual
	) &&
	cat >expect <<-\EOF &&
	dir/two
	one
	EOF
	grit ls-files >actual &&
	test_cmp expect actual
'

test_expect_success 'cat-file from subdirectory reads same blob' '
	cd repo &&
	two_oid=$(grit ls-files -s dir/two | awk "{print \$2}") &&
	grit cat-file -p "$two_oid" >actual &&
	test_cmp dir/two actual &&
	(
		cd dir &&
		grit cat-file -p "$two_oid" >actual &&
		test_cmp two actual
	)
'
rm -f actual dir/actual

test_expect_success 'write-tree returns same tree from subdirectory' '
	cd repo &&
	top=$(grit write-tree) &&
	(
		cd dir &&
		sub=$(grit write-tree) &&
		test "$top" = "$sub"
	)
'

test_expect_success 'checkout-index from subdirectory restores file' '
	cd repo &&
	echo changed >>dir/two &&
	(
		cd dir &&
		grit checkout-index -f -u two &&
		test_cmp two ../original.two
	)
'

test_expect_success 'read-tree --reset -u from subdirectory restores worktree' '
	cd repo &&
	tree=$(grit write-tree) &&
	rm -f one dir/two &&
	grit read-tree --reset -u "$tree" &&
	test_cmp one original.one &&
	test_cmp dir/two original.two &&
	(
		cd dir &&
		rm -f two &&
		grit read-tree --reset -u "$tree" &&
		test_cmp two ../original.two &&
		test_cmp ../one ../original.one
	)
'

test_done
