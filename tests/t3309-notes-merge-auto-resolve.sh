#!/bin/sh

test_description='Test notes add/remove/append resolution with same ref'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit 1st &&
	test_commit 2nd &&
	test_commit 3rd
'

test_expect_success 'add note, then force-overwrite' '
	git notes add -m "original" 1st &&
	git notes add -f -m "overwritten" 1st &&
	echo "overwritten" >expect &&
	git notes show 1st >actual &&
	test_cmp expect actual
'

test_expect_success 'append preserves original content' '
	git notes add -m "first line" 2nd &&
	git notes append -m "second line" 2nd &&
	cat >expect <<-\EOF &&
	first line

	second line
	EOF
	git notes show 2nd >actual &&
	test_cmp expect actual
'

test_expect_success 'multiple appends accumulate' '
	git notes add -m "line1" 3rd &&
	git notes append -m "line2" 3rd &&
	git notes append -m "line3" 3rd &&
	cat >expect <<-\EOF &&
	line1

	line2

	line3
	EOF
	git notes show 3rd >actual &&
	test_cmp expect actual
'

test_done
