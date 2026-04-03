#!/bin/sh
#
# Ported from git/t/t2019-checkout-ambiguous-ref.sh

test_description='checkout handling of ambiguous (branch/tag) refs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'setup ambiguous refs' '
	echo branch >file &&
	git add file &&
	test_tick &&
	git commit -m branch &&
	git tag branch &&
	git branch ambiguity &&
	git branch vagueness &&
	echo tag >file &&
	git add file &&
	test_tick &&
	git commit -m tag &&
	git tag tag &&
	git tag -f ambiguity HEAD &&
	echo other >file &&
	git add file &&
	test_tick &&
	git commit -m other &&
	git tag other
'

test_expect_success 'checkout ambiguous ref succeeds and chooses branch' '
	git checkout ambiguity >output 2>&1 &&
	echo refs/heads/ambiguity >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual &&
	echo branch >expect &&
	test_cmp expect file &&
	grep "Switched to branch" output
'

test_expect_success 'checkout vague ref succeeds and chooses branch' '
	git checkout master >output 2>&1 &&
	git checkout vagueness >output 2>&1 &&
	echo refs/heads/vagueness >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual &&
	grep "Switched to branch" output
'

test_done
