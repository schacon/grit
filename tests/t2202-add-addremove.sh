#!/bin/sh

test_description='git add --all'

. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_success setup '
	(
		echo .gitignore &&
		echo will-remove
	) >expect &&
	(
		echo .bin &&
		echo actual &&
		echo expect &&
		echo ignored
	) >.gitignore &&
	git add --all &&
	>will-remove &&
	git add --all &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	test_tick &&
	git commit -m initial &&
	git ls-files >actual &&
	test_cmp expect actual
'

test_expect_success 'Just "git add" is a no-op' '
	echo >will-remove &&
	>will-not-be-added &&
	git add
'

test_done
