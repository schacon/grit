#!/bin/sh

test_description='Test notes with multiple refs (simulating merge scenarios)'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit A &&
	test_commit B &&
	test_commit C
'

test_expect_success 'add notes with different refs' '
	git notes add -m "note from ref1" A &&
	git notes --ref=other add -m "note from ref2" A
'

test_expect_success 'default ref shows default note' '
	echo "note from ref1" >expect &&
	git notes show A >actual &&
	test_cmp expect actual
'

test_expect_success 'other ref shows other note' '
	echo "note from ref2" >expect &&
	git notes --ref=other show A >actual &&
	test_cmp expect actual
'

test_expect_success 'notes on different commits with same ref' '
	git notes --ref=work add -m "work note B" B &&
	git notes --ref=work add -m "work note C" C &&
	echo "work note B" >expect &&
	git notes --ref=work show B >actual &&
	test_cmp expect actual &&
	echo "work note C" >expect &&
	git notes --ref=work show C >actual &&
	test_cmp expect actual
'

test_expect_success 'list notes from specific ref' '
	git notes --ref=work list >actual &&
	test_line_count = 2 actual
'

test_done
