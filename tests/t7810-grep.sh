#!/bin/sh
test_description='git grep'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	cat >file <<-\EOF &&
	foo mmap bar
	foo_mmap bar mmap
	foo mmap bar_mmap
	foo mmap bar mmap baz
	EOF
	cat >hello_world <<-\EOF &&
	Hello world
	HeLLo world
	Hello_world
	EOF
	cat >space <<-\EOF &&
	line with leading space1
	line with leading space2
	line with leading space3
	EOF
	git add . &&
	git commit -m "initial"
'

test_expect_success 'grep -w pattern' '
	cd repo &&
	git grep -n -w "foo" >actual &&
	test -s actual
'

test_expect_success 'grep -c shows count' '
	cd repo &&
	git grep -c "mmap" >actual &&
	grep "file:" actual
'

test_expect_success 'grep pattern in specific file' '
	cd repo &&
	git grep "Hello" hello_world >actual &&
	test_line_count = 2 actual
'

test_expect_success 'grep -i case insensitive' '
	cd repo &&
	git grep -i "hello" hello_world >actual &&
	test_line_count = 3 actual
'

test_expect_success 'grep -l shows only filenames' '
	cd repo &&
	git grep -l "mmap" >actual &&
	echo file >expect &&
	test_cmp expect actual
'

test_expect_success 'grep in tree-ish' '
	cd repo &&
	git grep "mmap" HEAD >actual &&
	test -s actual
'

test_expect_success 'grep with no match returns non-zero' '
	cd repo &&
	test_must_fail git grep "nonexistent_pattern_xyz"
'

test_done
