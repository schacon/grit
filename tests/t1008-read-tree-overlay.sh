#!/bin/sh
# Ported subset from git/t/t1008-read-tree-overlay.sh.

test_description='grit read-tree multi-tree overlay without merge'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup three trees and refs' '
	grit init repo &&
	cd repo &&
	echo one >a &&
	grit update-index --add a &&
	tree_initial=$(grit write-tree) &&
	commit_initial=$(echo initial | grit commit-tree "$tree_initial") &&
	grit update-ref refs/heads/initial "$commit_initial" &&
	echo two >b &&
	grit update-index --add b &&
	tree_main=$(grit write-tree) &&
	commit_main=$(echo main | grit commit-tree "$tree_main" -p "$commit_initial") &&
	grit update-ref refs/heads/main "$commit_main" &&
	echo three >a &&
	rm -f b &&
	mkdir b &&
	echo four >b/c &&
	grit update-index --force-remove b &&
	grit update-index --add a b/c &&
	tree_side=$(grit write-tree) &&
	commit_side=$(echo side | grit commit-tree "$tree_side" -p "$commit_initial") &&
	grit update-ref refs/heads/side "$commit_side"
'

test_expect_success 'overlay initial main side yields expected paths' '
	cd repo &&
	rm -f .git/index &&
	grit read-tree initial main side &&
	grit ls-files >actual &&
	cat >expect <<-\EOF &&
	a
	b/c
	EOF
	test_cmp expect actual
'

test_done
