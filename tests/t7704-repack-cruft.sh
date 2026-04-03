#!/bin/sh

test_description='git repack cruft pack operations (grit verification)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repack-cruft &&
	cd repack-cruft &&

	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m base &&
	git tag base &&

	echo change1 >file &&
	git add file &&
	test_tick &&
	git commit -m change1 &&

	echo change2 >file &&
	git add file &&
	test_tick &&
	git commit -m change2
'

test_expect_success 'grit reads repo with loose objects' '
	cd repack-cruft &&
	git log --oneline >output &&
	test $(wc -l <output) -eq 3
'

test_expect_success 'grit reads tree' '
	cd repack-cruft &&
	git ls-tree HEAD >output &&
	grep file output
'

test_expect_success 'grit reads old commits via tag' '
	cd repack-cruft &&
	git cat-file commit base >output &&
	grep "base" output
'

test_expect_success 'grit diff between commits' '
	cd repack-cruft &&
	git diff base HEAD >output &&
	grep "change2" output
'

test_done
