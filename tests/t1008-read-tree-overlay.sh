#!/bin/sh
# Ported subset from git/t/t1008-read-tree-overlay.sh.

test_description='gust read-tree multi-tree overlay without merge'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup three trees and refs' '
	gust init repo &&
	cd repo &&
	echo one >a &&
	gust update-index --add a &&
	tree_initial=$(gust write-tree) &&
	commit_initial=$(echo initial | gust commit-tree "$tree_initial") &&
	gust update-ref refs/heads/initial "$commit_initial" &&
	echo two >b &&
	gust update-index --add b &&
	tree_main=$(gust write-tree) &&
	commit_main=$(echo main | gust commit-tree "$tree_main" -p "$commit_initial") &&
	gust update-ref refs/heads/main "$commit_main" &&
	echo three >a &&
	rm -f b &&
	mkdir b &&
	echo four >b/c &&
	gust update-index --force-remove b &&
	gust update-index --add a b/c &&
	tree_side=$(gust write-tree) &&
	commit_side=$(echo side | gust commit-tree "$tree_side" -p "$commit_initial") &&
	gust update-ref refs/heads/side "$commit_side"
'

test_expect_success 'overlay initial main side yields expected paths' '
	cd repo &&
	rm -f .git/index &&
	gust read-tree initial main side &&
	gust ls-files >actual &&
	cat >expect <<-\EOF &&
	a
	b/c
	EOF
	test_cmp expect actual
'

test_done
