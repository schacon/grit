#!/bin/sh

test_description='git rebase with file additions on different branches'

. ./test-lib.sh

test_expect_success 'setup: branches with different files' '
	git init -q &&
	echo base >file &&
	git add file &&
	git commit -m "base" &&

	git branch A &&
	git branch B &&

	git checkout A &&
	echo "new-content" >newfile-a &&
	git add newfile-a &&
	git commit -m "Add newfile-a on branch A" &&

	git checkout B &&
	echo "other-content" >newfile-b &&
	git add newfile-b &&
	git commit -m "Add newfile-b on branch B"
'

test_expect_success 'rebase B onto A succeeds' '
	git checkout B &&
	git rebase A &&
	test -f newfile-a &&
	test -f newfile-b &&
	test -f file
'

test_expect_success 'rebased commit is on top of A' '
	git rev-parse A >expect &&
	git rev-parse HEAD^ >actual &&
	test_cmp expect actual
'

test_expect_success 'content preserved after rebase' '
	echo "other-content" >expect &&
	test_cmp expect newfile-b &&
	echo "new-content" >expect &&
	test_cmp expect newfile-a
'

test_done
