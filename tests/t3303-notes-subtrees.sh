#!/bin/sh

test_description='Test commit notes organized in subtrees'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup: create commits and notes' '
	git init -q &&
	i=0 &&
	while test $i -lt 10
	do
		i=$(($i + 1)) &&
		test_commit "commit$i" "file$i" "content$i" || return 1
	done &&
	i=0 &&
	while test $i -lt 10
	do
		i=$(($i + 1)) &&
		git notes add -f -m "note for commit #$i" "commit$i" || return 1
	done
'

test_expect_success 'notes show works for all commits' '
	i=0 &&
	while test $i -lt 10
	do
		i=$(($i + 1)) &&
		echo "note for commit #$i" >expect &&
		git notes show "commit$i" >actual &&
		test_cmp expect actual || return 1
	done
'

test_expect_success 'notes list shows all entries' '
	git notes list >actual &&
	test_line_count = 10 actual
'

test_expect_success 'notes ref has correct tree structure' '
	git ls-tree -r refs/notes/commits >actual &&
	test_line_count = 10 actual
'

test_done
