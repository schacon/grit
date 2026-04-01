#!/bin/sh
# Ported subset from git/t/t0008-ignores.sh.

test_description='grit check-ignore subset'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repository with ignore sources' '
	grit init repo &&
	cd repo &&
	echo "ref: refs/heads/main" >.git/HEAD &&
	mkdir -p a/b/ignored-dir .git/info &&
	cat >.gitignore <<-\EOF &&
	one
	ignored-*
	top-level-dir/
	EOF
	cat >a/.gitignore <<-\EOF &&
	two*
	*three
	EOF
	cat >a/b/.gitignore <<-\EOF &&
	four
	five
	# comment to affect line numbers
	six
	ignored-dir/
	# and blank line below also counts

	!on*
	!two
	EOF
	echo per-repo >.git/info/exclude &&
	cat >global-excludes <<-\EOF &&
	globalone
	!globaltwo
	globalthree
	EOF
	: >ignored-and-untracked &&
	: >a/ignored-and-untracked &&
	: >ignored-but-in-index &&
	: >a/ignored-but-in-index &&
	grit update-index --add ignored-but-in-index a/ignored-but-in-index
'

test_expect_success 'empty command line fails' '
	cd repo &&
	test_must_fail grit check-ignore >out 2>err &&
	grep "no path specified" err
'

test_expect_success '--stdin with extra path fails' '
	cd repo &&
	test_must_fail grit check-ignore --stdin foo >out 2>err &&
	grep "cannot specify pathnames with --stdin" err
'

test_expect_success '-z without --stdin fails' '
	cd repo &&
	test_must_fail grit check-ignore -z >out 2>err &&
	grep -- "-z only makes sense with --stdin" err
'

test_expect_success 'basic path arguments and verbose output' '
	cd repo &&
	grit check-ignore one not-ignored >actual &&
	echo one >expect &&
	test_cmp expect actual &&
	grit check-ignore -v one >actual &&
	echo ".gitignore:1:one	one" >expect &&
	test_cmp expect actual
'

test_expect_success 'tracked paths hidden unless --no-index' '
	cd repo &&
	test_must_fail grit check-ignore ignored-but-in-index >actual 2>err &&
	test ! -s actual &&
	grit check-ignore --no-index ignored-but-in-index >actual &&
	echo ignored-but-in-index >expect &&
	test_cmp expect actual
'

test_expect_success 'nested gitignore negation visible with verbose' '
	cd repo &&
	test_must_fail grit check-ignore a/b/one >actual 2>err &&
	test ! -s actual &&
	grit check-ignore -v a/b/one >actual &&
	echo "a/b/.gitignore:8:!on*	a/b/one" >expect &&
	test_cmp expect actual
'

test_expect_success 'directory pattern applies to directory and descendants' '
	cd repo &&
	grit check-ignore a/b/ignored-dir a/b/ignored-dir/file >actual &&
	cat >expect <<-\EOF &&
	a/b/ignored-dir
	a/b/ignored-dir/file
	EOF
	test_cmp expect actual &&
	grit check-ignore -v a/b/ignored-dir/file >actual &&
	echo "a/b/.gitignore:5:ignored-dir/	a/b/ignored-dir/file" >expect &&
	test_cmp expect actual
'

test_expect_success '--stdin default mode' '
	cd repo &&
	cat >stdin <<-\EOF &&
	one
	not-ignored
	a/b/twooo
	EOF
	grit check-ignore --stdin <stdin >actual &&
	cat >expect <<-\EOF &&
	one
	a/b/twooo
	EOF
	test_cmp expect actual
'

test_expect_success '--stdin verbose non-matching mode' '
	cd repo &&
	cat >stdin <<-\EOF &&
	one
	not-ignored
	a/b/twooo
	EOF
	grit check-ignore --stdin -v -n <stdin >actual &&
	cat >expect <<-\EOF &&
	.gitignore:1:one	one
	::	not-ignored
	a/.gitignore:1:two*	a/b/twooo
	EOF
	test_cmp expect actual
'

test_expect_success '--stdin -z emits NUL-delimited records' '
	cd repo &&
	printf "one\0not-ignored\0a/b/twooo\0" >stdin0 &&
	grit check-ignore --stdin -z <stdin0 >actual0 &&
	printf "one\0a/b/twooo\0" >expect0 &&
	test_cmp expect0 actual0 &&
	grit check-ignore --stdin -z -v <stdin0 >actual0 &&
	printf ".gitignore\0001\000one\000one\000a/.gitignore\0001\000two*\000a/b/twooo\000" >expect0 &&
	test_cmp expect0 actual0
'

test_expect_success 'info/exclude and core.excludesfile precedence' '
	cd repo &&
	grit check-ignore -v per-repo a/per-repo >actual &&
	cat >expect <<-\EOF &&
	.git/info/exclude:1:per-repo	per-repo
	.git/info/exclude:1:per-repo	a/per-repo
	EOF
	test_cmp expect actual &&
	cat >>.git/config <<-\EOF &&
	[core]
		excludesfile = global-excludes
	EOF
	grit check-ignore -v globalone per-repo globalthree a/globalthree globaltwo >actual &&
	cat >expect <<-\EOF &&
	global-excludes:1:globalone	globalone
	.git/info/exclude:1:per-repo	per-repo
	global-excludes:3:globalthree	globalthree
	a/.gitignore:2:*three	a/globalthree
	global-excludes:2:!globaltwo	globaltwo
	EOF
	test_cmp expect actual
'

test_done
