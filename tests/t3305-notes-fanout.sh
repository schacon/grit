#!/bin/sh

test_description='Test commit notes fanout'

. ./test-lib.sh

test_expect_success 'setup: create many commits' '
	git init -q &&
	i=0 &&
	while test $i -lt 20
	do
		i=$(($i + 1)) &&
		test_commit "c$i" "f$i" "data$i" || return 1
	done
'

test_expect_success 'add notes to all commits' '
	i=0 &&
	while test $i -lt 20
	do
		i=$(($i + 1)) &&
		git notes add -f -m "note $i" "c$i" || return 1
	done
'

test_expect_success 'all notes readable' '
	i=0 &&
	while test $i -lt 20
	do
		i=$(($i + 1)) &&
		echo "note $i" >expect &&
		git notes show "c$i" >actual &&
		test_cmp expect actual || return 1
	done
'

test_expect_success 'notes list shows 20 entries' '
	git notes list >actual &&
	test_line_count = 20 actual
'

test_done
