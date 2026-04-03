#!/bin/sh

test_description='check diff-tree with nested directories'
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

make_dir() {
	mkdir -p "$1" &&
	echo "$2" >"$1/file"
}

make_files() {
	echo "$1" >file &&
	make_dir one "$1" &&
	make_dir one/two "$1" &&
	make_dir one/two/three "$1"
}

test_expect_success 'setup' '
	git commit --allow-empty -m empty &&
	git tag empty &&
	make_files added &&
	git add . &&
	git commit -m added &&
	git tag added_tag &&
	make_files modified &&
	git add . &&
	git commit -m modified &&
	git tag modified_tag
'

test_expect_success 'diff-tree shows all changed files recursively' '
	git diff-tree -r --name-only added_tag modified_tag >actual &&
	grep "file" actual &&
	grep "one/file" actual &&
	grep "one/two/file" actual &&
	grep "one/two/three/file" actual
'

test_expect_success 'diff-tree with --stat' '
	git diff-tree --stat -r added_tag modified_tag >actual &&
	grep "4 files changed" actual
'

test_expect_success 'diff-tree with -p shows patch' '
	git diff-tree -p added_tag modified_tag >actual &&
	grep "^-added" actual &&
	grep "^+modified" actual
'

test_done
