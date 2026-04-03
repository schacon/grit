#!/bin/sh

test_description='Test notes with multiple refs and many notes (fanout)'

. ./test-lib.sh

test_expect_success 'setup: create 15 commits' '
	git init -q &&
	i=0 &&
	while test $i -lt 15
	do
		i=$(($i + 1)) &&
		test_commit "c$i" "f$i" "data$i" || return 1
	done
'

test_expect_success 'add notes in two refs' '
	i=0 &&
	while test $i -lt 15
	do
		i=$(($i + 1)) &&
		git notes --ref=alpha add -f -m "alpha-$i" "c$i" &&
		git notes --ref=beta add -f -m "beta-$i" "c$i" || return 1
	done
'

test_expect_success 'both refs have 15 notes' '
	git notes --ref=alpha list >actual &&
	test_line_count = 15 actual &&
	git notes --ref=beta list >actual &&
	test_line_count = 15 actual
'

test_expect_success 'notes are independent between refs' '
	echo "alpha-5" >expect &&
	git notes --ref=alpha show c5 >actual &&
	test_cmp expect actual &&
	echo "beta-5" >expect &&
	git notes --ref=beta show c5 >actual &&
	test_cmp expect actual
'

test_done
