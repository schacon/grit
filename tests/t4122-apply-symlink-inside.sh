#!/bin/sh

test_description='apply to deeper directory without path confusion'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init repo && cd repo &&
	test_write_lines 1 2 3 4 5 >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag base &&

	test_write_lines 2 3 4 5 6 >file &&
	git add file &&
	test_tick &&
	git commit -a -m second &&
	git tag new
'

test_expect_success 'generate and apply patch' '
	cd repo &&
	git diff-tree -p base new >test.patch &&
	git checkout base &&
	git apply test.patch &&
	test_write_lines 2 3 4 5 6 >expect &&
	test_cmp expect file
'

test_expect_success 'check result matches new tree' '
	cd repo &&
	git add file &&
	test_tick &&
	git commit -m replay &&
	T1=$(git rev-parse "new^{tree}") &&
	T2=$(git rev-parse "HEAD^{tree}") &&
	test "z$T1" = "z$T2"
'

test_done
