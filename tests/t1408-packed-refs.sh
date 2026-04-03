#!/bin/sh

test_description='packed-refs entries are covered by loose refs'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success setup '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	test_tick &&
	git commit --allow-empty -m one &&
	one=$(git rev-parse HEAD) &&
	git for-each-ref >actual &&
	echo "$one commit	refs/heads/main" >expect &&
	test_cmp expect actual
'

test_expect_success 'pack-refs --all and for-each-ref still works' '
	one=$(git rev-parse HEAD) &&
	git pack-refs --all &&
	git for-each-ref >actual &&
	echo "$one commit	refs/heads/main" >expect &&
	test_cmp expect actual
'

test_expect_success 'new commit overrides packed ref' '
	test_tick &&
	git commit --allow-empty -m two &&
	two=$(git rev-parse HEAD) &&
	git for-each-ref >actual &&
	echo "$two commit	refs/heads/main" >expect &&
	test_cmp expect actual
'

test_done
