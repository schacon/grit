#!/bin/sh

test_description='checkout handling of ambiguous (branch/tag) refs'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup ambiguous refs' '
	test_commit branch file &&
	git branch ambiguity &&
	test_commit tag file &&
	git tag ambiguity
'

test_expect_success 'checkout ambiguous ref succeeds' '
	git checkout ambiguity 2>stderr
'

test_expect_success 'checkout chooses branch over tag' '
	echo refs/heads/ambiguity >expect &&
	git symbolic-ref HEAD >actual &&
	test_cmp expect actual
'

test_done
