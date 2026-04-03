#!/bin/sh

test_description='rev-list object listing and counting'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'set up repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_commit one &&
	test_commit two &&
	test_commit three &&
	test_commit four
'

test_expect_success 'rev-list --objects HEAD' '
	cd repo &&
	git rev-list --objects HEAD >output &&
	test -s output
'

test_expect_success 'rev-list --objects --no-object-names' '
	cd repo &&
	git rev-list --objects --no-object-names HEAD >output &&
	test -s output &&
	! grep " " output
'

test_expect_success 'rev-list --count' '
	cd repo &&
	count=$(git rev-list --count HEAD) &&
	test "$count" = "4"
'

test_expect_success 'rev-list --objects lists at least commits' '
	cd repo &&
	commits=$(git rev-list HEAD | wc -l) &&
	objects=$(git rev-list --objects HEAD | wc -l) &&
	test "$objects" -ge "$commits"
'

test_done
