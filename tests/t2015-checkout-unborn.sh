#!/bin/sh
# Test checkout behavior on unborn branch (empty repo)

test_description='grit checkout on unborn branch'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup empty repo' '
	grit init empty &&
	cd empty &&
	git config user.name "Test" &&
	git config user.email "test@test.com"
'

test_expect_success 'HEAD points to master on unborn branch' '
	cd empty &&
	grit symbolic-ref HEAD >actual &&
	echo refs/heads/master >expect &&
	test_cmp expect actual
'

test_expect_success 'status works on unborn branch' '
	cd empty &&
	grit status >output 2>&1 &&
	grep -i "no commits yet" output
'

test_expect_success 'checkout -b switches unborn branch name' '
	cd empty &&
	grit checkout -b newbranch &&
	grit symbolic-ref HEAD >actual &&
	echo refs/heads/newbranch >expect &&
	test_cmp expect actual
'

test_expect_success 'branch list is empty on unborn branch' '
	cd empty &&
	grit branch >output 2>&1 &&
	test_must_be_empty output
'

test_expect_success 'checkout -b works multiple times on unborn' '
	cd empty &&
	grit checkout -b another &&
	grit symbolic-ref HEAD >actual &&
	echo refs/heads/another >expect &&
	test_cmp expect actual
'

test_expect_success 'first commit on renamed unborn branch works' '
	cd empty &&
	echo "content" >file.txt &&
	grit add file.txt &&
	grit commit -m "first commit" &&
	grit rev-parse HEAD >actual &&
	test -s actual
'

test_expect_success 'branch now appears in list after first commit' '
	cd empty &&
	grit branch >output &&
	grep "another" output
'

test_expect_success 'checkout -b from existing commit works' '
	cd empty &&
	grit checkout -b frombranch &&
	grit symbolic-ref HEAD >actual &&
	echo refs/heads/frombranch >expect &&
	test_cmp expect actual
'

test_done
