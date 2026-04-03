#!/bin/sh

test_description='Examples from the git-notes man page

Make sure the manual is not full of lies.'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit A &&
	test_commit B &&
	test_commit C
'

test_expect_success 'example 1: notes to add an Acked-by line' '
	git notes add -m "Acked-by: A C Ker <acker@example.com>" B &&
	git notes show B >actual &&
	echo "Acked-by: A C Ker <acker@example.com>" >expect &&
	test_cmp expect actual
'

test_done
