#!/bin/sh

test_description='Test notes with mixed operations'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit A &&
	test_commit B &&
	test_commit C &&
	test_commit D
'

test_expect_success 'add notes to multiple commits' '
	git notes add -m "note A" A &&
	git notes add -m "note B" B &&
	git notes add -m "note C" C
'

test_expect_success 'verify each note' '
	echo "note A" >expect && git notes show A >actual && test_cmp expect actual &&
	echo "note B" >expect && git notes show B >actual && test_cmp expect actual &&
	echo "note C" >expect && git notes show C >actual && test_cmp expect actual
'

test_expect_success 'no note on D' '
	test_must_fail git notes show D
'

test_expect_success 'list shows 3 notes' '
	git notes list >actual &&
	test_line_count = 3 actual
'

test_expect_success 'remove note from B' '
	git notes remove B &&
	test_must_fail git notes show B
'

test_expect_success 'list shows 2 notes after removal' '
	git notes list >actual &&
	test_line_count = 2 actual
'

test_expect_success 'append to note on A' '
	git notes append -m "extra for A" A &&
	cat >expect <<-\EOF &&
	note A

	extra for A
	EOF
	git notes show A >actual &&
	test_cmp expect actual
'

test_expect_success 'overwrite note on C with force' '
	git notes add -f -m "new note C" C &&
	echo "new note C" >expect &&
	git notes show C >actual &&
	test_cmp expect actual
'

test_done
