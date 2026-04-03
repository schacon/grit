#!/bin/sh
#
# Ported from upstream git t3301-notes.sh
#

test_description='Test commit notes'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit 1st &&
	test_commit 2nd
'

test_expect_success 'cannot annotate non-existing HEAD in empty repo' '
	(
		mkdir empty &&
		cd empty &&
		git init -q &&
		test_must_fail git notes add -m "note"
	)
'

test_expect_success 'handle empty notes gracefully' '
	test_must_fail git notes show
'

test_expect_success 'create notes with -m' '
	git notes add -m "b4" &&
	git ls-tree -r refs/notes/commits >actual &&
	test_line_count = 1 actual &&
	echo b4 >expect &&
	git notes show >actual &&
	test_cmp expect actual
'

test_expect_success 'show notes for HEAD' '
	echo b4 >expect &&
	git notes show HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'cannot add notes where notes already exists' '
	test_must_fail git notes add -m "b2"
'

test_expect_success 'can overwrite existing note with -f -m' '
	git notes add -f -m "b1" &&
	echo b1 >expect &&
	git notes show >actual &&
	test_cmp expect actual
'

test_expect_success 'notes show HEAD^ fails (no note)' '
	test_must_fail git notes show HEAD^
'

test_expect_success 'notes list shows the mapping' '
	git notes list >actual &&
	test_line_count = 1 actual
'

test_expect_success 'remove note' '
	git notes remove &&
	test_must_fail git notes show
'

test_expect_success 'add note to specific commit' '
	git notes add -m "note on 1st" HEAD^ &&
	echo "note on 1st" >expect &&
	git notes show HEAD^ >actual &&
	test_cmp expect actual
'

test_expect_success 'remove note from specific commit' '
	git notes remove HEAD^ &&
	test_must_fail git notes show HEAD^
'

test_expect_success 'add another note with -m' '
	git notes add -m "another-note" &&
	echo "another-note" >expect &&
	git notes show >actual &&
	test_cmp expect actual &&
	git notes remove
'

test_expect_success 'append to non-existing note' '
	git notes append -m "first append" &&
	echo "first append" >expect &&
	git notes show >actual &&
	test_cmp expect actual
'

test_expect_success 'append to existing note' '
	git notes append -m "second append" &&
	cat >expect <<-\EOF &&
	first append

	second append
	EOF
	git notes show >actual &&
	test_cmp expect actual &&
	git notes remove
'

test_expect_success 'notes with --ref' '
	git notes --ref=other add -m "other note" &&
	echo "other note" >expect &&
	git notes --ref=other show >actual &&
	test_cmp expect actual &&
	test_must_fail git notes show
'

test_expect_success 'notes list with --ref' '
	git notes --ref=other list >actual &&
	test_line_count = 1 actual
'

test_expect_success 'notes --ref=other still works after all operations' '
	echo "other note" >expect &&
	git notes --ref=other show >actual &&
	test_cmp expect actual
'

test_done
