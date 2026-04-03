#!/bin/sh

test_description='Tests git rev-list functionality'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup linear history' '
	test_commit A &&
	test_commit B &&
	test_commit C &&
	test_commit D &&
	test_commit E
'

test_expect_success 'rev-list shows all commits' '
	git rev-list HEAD >actual &&
	test_line_count = 5 actual
'

test_expect_success 'rev-list with exclusion' '
	git rev-list HEAD ^HEAD~2 >actual &&
	test_line_count = 2 actual
'

test_expect_success 'rev-list --count' '
	git rev-list --count HEAD >actual &&
	echo 5 >expect &&
	test_cmp expect actual
'

test_expect_success 'rev-list --reverse' '
	git rev-list --reverse HEAD >actual &&
	head -n1 actual >first &&
	git rev-parse HEAD~4 >expect &&
	test_cmp expect first
'

test_done
