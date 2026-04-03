#!/bin/sh

test_description='test dwim of revs versus pathspecs in revision parser'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_commit base
'

test_expect_success 'log with pathspec on tracked file' '
	cd repo &&
	git log --oneline -- base.t >actual &&
	test_line_count = 1 actual
'

test_expect_success 'log with no-match pathspec is empty' '
	cd repo &&
	git log --oneline -- nonexistent >actual &&
	test_must_be_empty actual
'

test_expect_success 'rev-parse resolves HEAD' '
	cd repo &&
	git rev-parse HEAD >actual &&
	test -s actual
'

test_done
