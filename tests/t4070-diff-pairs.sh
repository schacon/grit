#!/bin/sh

test_description='basic diff-tree and diff output tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo to-be-gone >deleted &&
	echo original >modified &&
	echo content >unchanged &&
	git add . &&
	test_tick &&
	git commit -m base &&
	git tag base &&

	echo now-here >added &&
	echo new >modified &&
	rm deleted &&
	git add -A . &&
	test_tick &&
	git commit -m new &&
	git tag new
'

test_expect_success 'diff-tree shows changes between commits' '
	cd repo &&
	git diff-tree -r base new >output &&
	test_grep "modified" output &&
	test_grep "deleted" output &&
	test_grep "added" output
'

test_expect_success 'diff-tree -p shows patch output' '
	cd repo &&
	git diff-tree -p base new >output &&
	grep "diff --git" output &&
	grep "+now-here" output &&
	grep "+new" output &&
	grep "\-to-be-gone" output
'

test_expect_success 'diff-tree with single commit shows parent diff' '
	cd repo &&
	git diff-tree -r new >output &&
	test_grep "modified" output
'

test_done
