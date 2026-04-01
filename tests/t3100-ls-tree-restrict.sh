#!/bin/sh
# Ported from git/t/t3100-ls-tree-restrict.sh (harness-compatible subset).

test_description='gust ls-tree restrict'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

normalize_output() {
	sed -E 's/ [0-9a-f]{40}\t/ X\t/' <"$1" >"$2"
}

test_expect_success 'setup fixture tree' '
	gust init repo &&
	cd repo &&
	mkdir -p path2/baz &&
	echo Hi >path0 &&
	ln -s path0 path1 &&
	ln -s ../path1 path2/bazbo &&
	echo Lo >path2/foo &&
	echo Mi >path2/baz/b &&
	gust update-index --add path0 path1 path2/foo path2/bazbo path2/baz/b &&
	tree=$(gust write-tree) &&
	echo "$tree" >../tree_oid
'

test_expect_success 'ls-tree plain' '
	cd repo &&
	gust ls-tree "$(cat ../tree_oid)" >current &&
	cat >expected <<-\EOF &&
	100644 blob X	path0
	120000 blob X	path1
	040000 tree X	path2
	EOF
	normalize_output current check &&
	test_cmp expected check
'

test_expect_success 'ls-tree recursive with -t' '
	cd repo &&
	gust ls-tree -r -t "$(cat ../tree_oid)" >current &&
	cat >expected <<-\EOF &&
	100644 blob X	path0
	120000 blob X	path1
	040000 tree X	path2
	040000 tree X	path2/baz
	100644 blob X	path2/baz/b
	120000 blob X	path2/bazbo
	100644 blob X	path2/foo
	EOF
	normalize_output current check &&
	test_cmp expected check
'

test_expect_success 'ls-tree filtered with path2' '
	cd repo &&
	gust ls-tree "$(cat ../tree_oid)" path2 >current &&
	cat >expected <<-\EOF &&
	040000 tree X	path2
	EOF
	normalize_output current check &&
	test_cmp expected check
'

test_done
