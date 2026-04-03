#!/bin/sh

test_description='am conflict resolution tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo base >file &&
	git add file &&
	git commit -m base &&
	git tag base &&

	echo no-conflict >file &&
	git commit -a -m "no-conflict" &&
	git format-patch -1 --stdout >clean.patch &&

	git reset --hard base &&
	echo conflict-change >file &&
	git commit -a -m "conflict-main" &&
	git tag conflict-main
'

test_expect_success 'am applies clean patch' '
	cd repo &&
	git reset --hard base &&
	git am clean.patch &&
	echo no-conflict >expect &&
	test_cmp expect file
'

test_expect_success 'am conflicts on incompatible base' '
	cd repo &&
	git reset --hard conflict-main &&
	test_must_fail git am clean.patch &&
	test_path_is_dir .git/rebase-apply
'

test_expect_success 'am --abort after conflict' '
	cd repo &&
	git am --abort &&
	test_path_is_missing .git/rebase-apply &&
	echo conflict-change >expect &&
	test_cmp expect file
'

test_done
