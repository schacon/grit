#!/bin/sh

test_description='test handling of --alternate-refs traversal'

. ./test-lib.sh

test_expect_success 'set up local refs' '
	git init &&
	test_tick &&
	git commit --allow-empty -m base &&
	git checkout -b one &&
	test_tick &&
	git commit --allow-empty -m one &&
	git checkout -b two HEAD^ &&
	test_tick &&
	git commit --allow-empty -m two
'

test_expect_success 'set up clone' '
	git clone . child
'

test_expect_success 'rev-list --alternate-refs' '
	(
		cd child &&
		git rev-list --alternate-refs >actual &&
		test -s actual
	)
'

test_done
