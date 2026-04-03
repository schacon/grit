#!/bin/sh

test_description='basic pack and repository operations (request-pull area)'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	echo "initial" >file &&
	git add file &&
	test_tick &&
	git commit -m "initial commit"
'

test_expect_success 'rev-parse works' '
	git rev-parse HEAD >actual &&
	test $(wc -c <actual) -ge 40
'

test_expect_success 'log shows commit' '
	git log --oneline >actual &&
	test_line_count = 1 actual
'

test_expect_success 'branch and tag operations' '
	git branch feature &&
	git tag v1.0 &&
	git rev-parse feature >branch_oid &&
	git rev-parse v1.0 >tag_oid &&
	test_cmp branch_oid tag_oid
'

test_done
