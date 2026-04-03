#!/bin/sh

test_description='Test commit notes removal (prune-like behavior)'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit A &&
	test_commit B &&
	test_commit C
'

test_expect_success 'add notes to all' '
	git notes add -m "note A" A &&
	git notes add -m "note B" B &&
	git notes add -m "note C" C &&
	git notes list >actual &&
	test_line_count = 3 actual
'

test_expect_success 'remove notes one by one' '
	git notes remove A &&
	git notes list >actual &&
	test_line_count = 2 actual &&
	git notes remove B &&
	git notes list >actual &&
	test_line_count = 1 actual &&
	git notes remove C &&
	git notes list >actual &&
	test_line_count = 0 actual
'

test_expect_success 'removed notes no longer show' '
	test_must_fail git notes show A &&
	test_must_fail git notes show B &&
	test_must_fail git notes show C
'

test_done
