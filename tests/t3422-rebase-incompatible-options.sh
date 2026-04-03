#!/bin/sh

test_description='test if rebase detects and aborts on incompatible options'

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&

	test_seq 2 9 >foo &&
	git add foo &&
	git commit -m orig &&

	git branch A &&
	git branch B &&

	git checkout A &&
	test_seq 1 9 >foo &&
	git add foo &&
	git commit -m A &&

	git checkout B &&
	test_seq 2 10 >foo &&
	git add foo &&
	git commit -m B
'

test_expect_success 'rebase --continue without rebase in progress fails' '
	git checkout B^0 &&
	test_must_fail git rebase --continue 2>err &&
	grep -i "no rebase" err || grep -i "not in" err || grep -i "No rebase in progress" err
'

test_expect_success 'rebase --abort without rebase in progress fails' '
	git checkout B^0 &&
	test_must_fail git rebase --abort 2>err &&
	grep -i "no rebase" err || grep -i "not in" err || grep -i "No rebase in progress" err
'

test_expect_success 'rebase --skip without rebase in progress fails' '
	git checkout B^0 &&
	test_must_fail git rebase --skip 2>err &&
	grep -i "no rebase" err || grep -i "not in" err || grep -i "No rebase in progress" err
'

test_expect_success 'basic rebase works' '
	git checkout B^0 &&
	git rebase A
'

test_done
