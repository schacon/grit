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

test_expect_success 'hash-object from subdirectory sees same blob' '
	cd repo &&
	one_oid=$(grit hash-object one) &&
	(
		cd dir &&
		sub_oid=$(grit hash-object ../one) &&
		test "$one_oid" = "$sub_oid"
	)
'

# SKIP: rev-parse from inside .git not yet supported
# test_expect_success 'rev-parse HEAD works from inside .git'

test_expect_success 'diff-files from subdirectory' '
	cd repo &&
	echo a >>one &&
	echo d >>dir/two &&
	grit diff-files --name-only >actual &&
	# Should show both files regardless of cwd
	grep one actual &&
	grep dir/two actual &&
	(
		cd dir &&
		grit diff-files --name-only >actual &&
		# From subdir, should still see full paths
		grep one actual &&
		grep dir/two actual
	) &&
	# Restore files
	grit checkout-index -f -u one dir/two
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

test_expect_success 'ls-files from subdirectory shows relative paths' '
	cd repo &&
	(
		cd dir &&
		grit ls-files >actual &&
		cat >expect <<-\EOF &&
		two
		EOF
		test_cmp expect actual
	)
'

test_expect_success 'update-index --add from subdirectory with new file' '
	cd repo &&
	echo "new content" >dir/three &&
	(
		cd dir &&
		grit update-index --add three
	) &&
	grit ls-files >actual &&
	grep dir/three actual
'

test_expect_success 'no file/rev ambiguity check inside .git' '
	cd repo &&
	grit config user.name "Test User" &&
	grit config user.email "test@example.com" &&
	grit commit -q -m "test ambiguity" &&
	(
		cd .git &&
		grit rev-parse HEAD
	)
'

test_done
