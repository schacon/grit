#!/bin/sh
# Adapted from git/t/t7112-reset-submodule.sh
# Tests reset in repos with various structure

test_description='reset operations'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init reset-repo &&
	cd reset-repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&

	echo "main" >main.txt &&
	mkdir subdir &&
	echo "sub" >subdir/file &&
	git add main.txt subdir/file &&
	git commit -m "initial" &&
	git tag initial &&

	echo "modified" >>main.txt &&
	echo "modified" >>subdir/file &&
	git add main.txt subdir/file &&
	git commit -m "modified" &&
	git tag modified
'

test_expect_success 'reset --hard restores files' '
	cd reset-repo &&
	git reset --hard initial &&
	echo "main" >expect &&
	test_cmp expect main.txt &&
	echo "sub" >expect &&
	test_cmp expect subdir/file
'

test_expect_success 'reset --soft moves HEAD only' '
	cd reset-repo &&
	git reset --hard modified &&
	git reset --soft initial &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse initial)" &&
	# index should still have modified content
	git diff --cached --name-only >changed &&
	test_grep "main.txt" changed
'

test_expect_success 'reset --keep refuses when dirty' '
	cd reset-repo &&
	git reset --hard modified &&
	echo "dirty" >>main.txt &&
	test_must_fail git reset --keep initial
'

test_done
