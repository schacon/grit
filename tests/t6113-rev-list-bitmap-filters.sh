#!/bin/sh

test_description='rev-list with packed objects'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'set up repo' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_commit one &&
	test_commit two &&
	test_commit three &&
	git tag tag
'

test_expect_success 'rev-list --objects' '
	cd repo &&
	git rev-list --objects HEAD >output &&
	test -s output
'

test_expect_success 'rev-list --count' '
	cd repo &&
	git rev-list --count HEAD >output &&
	echo 3 >expect &&
	test_cmp expect output
'

test_expect_success 'rev-list with lightweight tag' '
	cd repo &&
	git rev-list tag >output &&
	test_line_count = 3 output
'

test_expect_success 'rev-list --objects lists at least commits' '
	cd repo &&
	git rev-list --objects HEAD >output &&
	test $(wc -l <output) -ge 3
'

test_expect_success 'pack-objects produces valid pack' '
	cd repo &&
	git rev-list HEAD | git pack-objects .git/objects/pack/test >name &&
	test -s name
'

test_done
