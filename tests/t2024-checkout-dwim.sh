#!/bin/sh
#
# Ported from git/t/t2024-checkout-dwim.sh (minimal subset)

test_description='checkout <branch> DWIM'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'setup' '
	test_commit my_main &&
	git init repo_a &&
	(
		cd repo_a &&
		test_commit a_main &&
		git checkout -b foo &&
		test_commit a_foo
	) &&
	git remote add repo_a repo_a &&
	git fetch --all
'

test_expect_success 'checkout of non-existing branch fails' '
	test_must_fail git checkout xyzzy 2>err &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'checkout of existing local branch works' '
	git checkout master >out 2>&1 &&
	echo refs/heads/master >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'checkout -B creates new branch' '
	git checkout master &&
	git checkout -B newbranch &&
	echo refs/heads/newbranch >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'checkout -B resets existing branch' '
	git checkout master &&
	git checkout -B newbranch &&
	echo refs/heads/newbranch >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_expect_success 'checkout -f resets working tree' '
	git checkout master &&
	echo clean >my_main.t &&
	git add my_main.t &&
	git commit -m "clean state" &&
	echo dirty >my_main.t &&
	git checkout -f &&
	echo clean >expect &&
	test_cmp expect my_main.t
'

test_done
