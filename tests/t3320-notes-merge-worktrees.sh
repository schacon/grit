#!/bin/sh

test_description='Test notes across different refs'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit A &&
	test_commit B
'

test_expect_success 'notes added with --ref persist' '
	git notes --ref=review add -m "reviewed" A &&
	echo "reviewed" >expect &&
	git notes --ref=review show A >actual &&
	test_cmp expect actual
'

test_expect_success 'default notes ref is separate' '
	git notes add -m "default note" A &&
	echo "default note" >expect &&
	git notes show A >actual &&
	test_cmp expect actual &&
	echo "reviewed" >expect &&
	git notes --ref=review show A >actual &&
	test_cmp expect actual
'

test_done
