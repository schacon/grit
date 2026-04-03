#!/bin/sh

test_description='git-am resume and option handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo initial >file &&
	git add file &&
	test_commit initial file &&

	echo first >file &&
	git commit -a -m first &&
	git tag first &&

	echo second >file &&
	git commit -a -m second &&
	git format-patch -1 --stdout >second.patch &&

	git reset --hard first &&
	echo conflicting >file &&
	git commit -a -m conflicting
'

test_expect_success 'am --abort after conflict restores state' '
	cd repo &&
	test_must_fail git am second.patch &&
	test_path_is_dir .git/rebase-apply &&
	git am --abort &&
	test_path_is_missing .git/rebase-apply &&
	echo conflicting >expect &&
	test_cmp expect file
'

test_expect_success 'am --skip after conflict skips patch' '
	cd repo &&
	test_must_fail git am second.patch &&
	git am --skip &&
	test_path_is_missing .git/rebase-apply
'

test_expect_success 'am with quiet mode' '
	cd repo &&
	git reset --hard initial &&
	echo change1 >file &&
	git commit -a -m change1 &&
	git format-patch -1 --stdout >quiet.patch &&
	git reset --hard initial &&
	git am --quiet quiet.patch >out 2>&1 &&
	test_must_be_empty out
'

test_done
