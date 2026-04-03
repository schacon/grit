#!/bin/sh
# Ported from upstream git t7800-difftool.sh
# Test diff operations that difftool would use

test_description='difftool (diff verification with grit)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init difftool-repo &&
	cd difftool-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo line1 >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	echo line2 >>file &&
	git add file &&
	test_tick &&
	git commit -m second
'

test_expect_success 'diff between commits' '
	cd difftool-repo &&
	git diff HEAD^ HEAD >actual &&
	grep "line2" actual
'

test_expect_success 'diff --stat between commits' '
	cd difftool-repo &&
	git diff --stat HEAD^ HEAD >actual &&
	grep "file" actual
'

test_expect_success 'diff --name-only' '
	cd difftool-repo &&
	git diff --name-only HEAD^ HEAD >actual &&
	echo "file" >expected &&
	test_cmp expected actual
'

test_expect_success 'diff --name-status' '
	cd difftool-repo &&
	git diff --name-status HEAD^ HEAD >actual &&
	grep "M" actual &&
	grep "file" actual
'

test_expect_success 'diff-tree between commits' '
	cd difftool-repo &&
	git diff-tree -p HEAD^ HEAD >actual &&
	grep "line2" actual
'

test_expect_success 'diff-tree --stat' '
	cd difftool-repo &&
	git diff-tree --stat HEAD^ HEAD >actual &&
	grep "file" actual
'

test_expect_success 'diff working tree' '
	cd difftool-repo &&
	echo line3 >>file &&
	git diff >actual &&
	grep "line3" actual
'

test_expect_success 'diff --cached' '
	cd difftool-repo &&
	git add file &&
	git diff --cached >actual &&
	grep "line3" actual
'

test_done
