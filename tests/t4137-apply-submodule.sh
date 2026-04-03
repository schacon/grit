#!/bin/sh

test_description='git apply with various patch formats'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo && cd repo &&
	echo base >file &&
	git add file &&
	git commit -m initial &&
	git tag initial
'

test_expect_success 'apply patch from git diff output' '
	cd repo &&
	echo changed >file &&
	git diff >patch.diff &&
	git checkout -- file &&
	git apply patch.diff &&
	echo changed >expect &&
	test_cmp expect file
'

test_expect_success 'apply --stat on git diff output' '
	cd repo &&
	git checkout -- file &&
	echo changed >file &&
	git diff >patch.diff &&
	git checkout -- file &&
	git apply --stat patch.diff >output &&
	test_grep "file" output &&
	test_grep "1 file changed" output
'

test_done
