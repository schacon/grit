#!/bin/sh

test_description='Test notes with separate refs, simulating manual resolution'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit A &&
	test_commit B
'

test_expect_success 'add notes in two separate refs' '
	git notes --ref=x add -m "x-note on A" A &&
	git notes --ref=y add -m "y-note on A" A &&
	git notes --ref=x add -m "x-note on B" B &&
	git notes --ref=y add -m "y-note on B" B
'

test_expect_success 'each ref has its own notes' '
	echo "x-note on A" >expect &&
	git notes --ref=x show A >actual &&
	test_cmp expect actual &&
	echo "y-note on A" >expect &&
	git notes --ref=y show A >actual &&
	test_cmp expect actual
'

test_expect_success 'update note in one ref does not affect other' '
	git notes --ref=x add -f -m "updated x-note on A" A &&
	echo "updated x-note on A" >expect &&
	git notes --ref=x show A >actual &&
	test_cmp expect actual &&
	echo "y-note on A" >expect &&
	git notes --ref=y show A >actual &&
	test_cmp expect actual
'

test_expect_success 'remove note in one ref does not affect other' '
	git notes --ref=x remove A &&
	test_must_fail git notes --ref=x show A &&
	echo "y-note on A" >expect &&
	git notes --ref=y show A >actual &&
	test_cmp expect actual
'

test_done
