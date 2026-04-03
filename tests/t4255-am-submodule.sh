#!/bin/sh

test_description='git am basic operation tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo one >file &&
	git add file &&
	git commit -m "one" &&
	git tag one &&

	echo two >file &&
	git commit -a -m "two" &&
	git format-patch -1 --stdout >two.patch &&

	echo three >file &&
	git commit -a -m "three" &&
	git format-patch -1 --stdout >three.patch
'

test_expect_success 'am applies single patch' '
	cd repo &&
	git reset --hard one &&
	git am two.patch &&
	echo two >expect &&
	test_cmp expect file
'

test_expect_success 'am applies patch and preserves commit message' '
	cd repo &&
	git log -n 1 --format=%s >actual &&
	echo "two" >expect &&
	test_cmp expect actual
'

test_expect_success 'am applies sequential patches' '
	cd repo &&
	git reset --hard one &&
	git am two.patch &&
	git am three.patch &&
	echo three >expect &&
	test_cmp expect file
'

test_expect_success 'am --dry-run does not modify anything' '
	cd repo &&
	git reset --hard one &&
	git am --dry-run two.patch &&
	echo one >expect &&
	test_cmp expect file
'

test_done
