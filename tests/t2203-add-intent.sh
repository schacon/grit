#!/bin/sh
#
# Ported from git/t/t2203-add-intent.sh (subset)

test_description='Intent to add'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'intent to add' '
	test_commit 1 &&
	git rm 1.t &&
	echo hello >1.t &&
	echo hello >file &&
	echo hello >elif &&
	git add -N file &&
	git add elif &&
	git add -N 1.t
'

test_expect_success 'intent to add is just an ordinary empty blob' '
	git add -u &&
	git ls-files -s file >actual &&
	git ls-files -s elif | sed -e "s/elif/file/" >expect &&
	test_cmp expect actual
'

test_expect_success 'intent to add does not clobber existing paths' '
	git add -N file elif &&
	empty=$(git hash-object --stdin </dev/null) &&
	git ls-files -s >actual &&
	! grep "$empty" actual
'

test_expect_success 'can "commit -a" with an i-t-a entry' '
	test_tick &&
	git commit -m initial &&
	git reset HEAD &&
	: >nitfol &&
	git add -N nitfol &&
	git commit -a -m all
'

test_done
