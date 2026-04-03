#!/bin/sh

test_description='git apply in reverse'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	test_write_lines a b c d e f g h i j k l m n >file1 &&
	git add file1 &&
	git commit -m initial &&
	git tag initial &&
	test_write_lines a b c g h i J K L m o n p q >file1 &&
	git commit -a -m second &&
	git tag second &&
	git diff initial second >patch
'

test_expect_success 'apply in forward' '
	git checkout initial &&
	git apply patch &&
	test_write_lines a b c g h i J K L m o n p q >expect &&
	test_cmp expect file1
'

test_expect_success 'apply in reverse' '
	git checkout -f second &&
	git apply --reverse patch &&
	test_write_lines a b c d e f g h i j k l m n >expect &&
	test_cmp expect file1
'

test_done
