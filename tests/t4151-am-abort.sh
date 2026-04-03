#!/bin/sh

test_description='am --abort'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	echo "original content" >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag initial &&

	echo "first change" >file &&
	test_tick &&
	git commit -a -m first &&
	git tag first &&
	git format-patch -1 --stdout >first.patch &&

	echo "second change" >file &&
	test_tick &&
	git commit -a -m second &&
	git format-patch -1 --stdout >second.patch &&
	git tag second &&

	git reset --hard first &&
	echo "conflicting change" >file &&
	test_tick &&
	git commit -a -m conflicting
'

test_expect_success 'am stops on conflict' '
	cd repo &&
	test_must_fail git am second.patch
'

test_expect_success 'am session is in progress' '
	cd repo &&
	test_path_is_dir .git/rebase-apply
'

test_expect_success 'am --abort restores HEAD' '
	cd repo &&
	git rev-parse HEAD >before &&
	git am --abort &&
	git rev-parse HEAD >after &&
	test_cmp before after
'

test_expect_success 'am --abort cleans up rebase-apply' '
	cd repo &&
	test_path_is_missing .git/rebase-apply
'

test_expect_success 'am --skip skips conflicting patch' '
	cd repo &&
	test_must_fail git am second.patch &&
	git am --skip &&
	test_path_is_missing .git/rebase-apply
'

test_expect_success 'am applies clean patch on matching base' '
	cd repo &&
	git reset --hard initial &&
	git am first.patch &&
	echo "first change" >expect &&
	test_cmp expect file
'

test_expect_success 'am applies multiple patches from files' '
	cd repo &&
	git reset --hard initial &&
	git am first.patch &&
	echo "first change" >expect &&
	test_cmp expect file &&
	git log -n 1 --format=%s >actual &&
	echo "first" >expect &&
	test_cmp expect actual
'

test_done
