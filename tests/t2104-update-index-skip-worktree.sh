#!/bin/sh
#
# Copyright (c) 2008 Nguyễn Thái Ngọc Duy
#

test_description='skip-worktree bit test'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	cat >expect.full <<-\EOF &&
	H 1
	H 2
	H sub/1
	H sub/2
	EOF

	cat >expect.skip <<-\EOF &&
	S 1
	H 2
	S sub/1
	H sub/2
	EOF

	mkdir sub &&
	touch ./1 ./2 sub/1 sub/2 &&
	git add 1 2 sub/1 sub/2 &&
	git ls-files -t | test_cmp expect.full -
'

test_expect_success 'update-index --skip-worktree' '
	git update-index --skip-worktree 1 sub/1 &&
	git ls-files -t | test_cmp expect.skip -
'

test_expect_success 'ls-files -t' '
	git ls-files -t | test_cmp expect.skip -
'

test_expect_success 'update-index --no-skip-worktree' '
	git update-index --no-skip-worktree 1 sub/1 &&
	git ls-files -t | test_cmp expect.full -
'

test_done
