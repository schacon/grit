#!/bin/sh

test_description='handling of missing objects in rev-list'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'create repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_commit 1 &&
	test_commit 2 &&
	test_commit 3 &&
	git tag -m "tag message" annot_tag HEAD~1 &&
	git tag regul_tag HEAD~1 &&
	git branch a_branch HEAD~1
'

test_expect_success 'rev-list basic traversal' '
	cd repo &&
	git rev-list HEAD >output &&
	test_line_count = 3 output
'

test_expect_success 'rev-list with lightweight tag' '
	cd repo &&
	git rev-list regul_tag >output &&
	test_line_count = 2 output
'

test_expect_success 'rev-list with branch' '
	cd repo &&
	git rev-list a_branch >output &&
	test_line_count = 2 output
'

test_expect_success 'rev-list range' '
	cd repo &&
	git rev-list HEAD~2..HEAD >output &&
	test_line_count = 2 output
'

test_expect_success 'rev-list with --objects' '
	cd repo &&
	git rev-list --objects HEAD >output &&
	test -s output
'

test_expect_success 'rev-list with --count' '
	cd repo &&
	git rev-list --count HEAD >output &&
	echo 3 >expect &&
	test_cmp expect output
'

test_done
