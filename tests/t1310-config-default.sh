#!/bin/sh

test_description='Test git config in different settings (with --default)'

. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success 'uses --default when entry missing' '
	echo quux >expect &&
	git config -f config get --default=quux core.foo >actual &&
	test_cmp expect actual
'

test_expect_success 'does not use --default when entry present' '
	echo bar >expect &&
	git -c core.foo=bar config get --default=baz core.foo >actual &&
	test_cmp expect actual
'

test_done
