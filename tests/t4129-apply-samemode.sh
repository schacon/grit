#!/bin/sh
test_description='applying patch with mode bits'
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success setup '
	echo original >file &&
	git add file &&
	test_tick &&
	git commit -m initial &&
	git tag initial &&
	echo modified >file &&
	git diff >patch-0.txt &&
	git checkout -- file
'

test_expect_success 'apply modifies content' '
	git reset --hard &&
	git apply patch-0.txt &&
	echo modified >expect &&
	test_cmp expect file
'

test_expect_success 'apply --check reports cleanly' '
	git reset --hard &&
	git apply --check patch-0.txt
'

test_expect_success 'apply --stat shows stats' '
	git apply --stat patch-0.txt >output &&
	test_grep "file" output
'

test_expect_success 'apply --numstat shows numstat' '
	git apply --numstat patch-0.txt >output &&
	test_grep "file" output
'

test_expect_success 'apply --summary on content-only patch' '
	git apply --summary patch-0.txt >output &&
	test_must_be_empty output
'

test_expect_success 'apply --reverse works' '
	git reset --hard &&
	git apply patch-0.txt &&
	echo modified >expect &&
	test_cmp expect file &&
	git apply -R patch-0.txt &&
	echo original >expect &&
	test_cmp expect file
'

test_done
