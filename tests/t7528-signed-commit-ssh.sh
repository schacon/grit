#!/bin/sh
# Ported from upstream git t7528-signed-commit-ssh.sh
# SSH signing not available, test commit/log structure

test_description='signed commit with SSH (structure tests, no SSH keys)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init ssh-signed-repo &&
	cd ssh-signed-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'unsigned commit has no gpgsig header' '
	cd ssh-signed-repo &&
	git cat-file -p HEAD >actual &&
	! grep "^gpgsig" actual
'

test_expect_success 'log shows commit without signature info' '
	cd ssh-signed-repo &&
	git log --oneline >actual &&
	test_line_count = 1 actual
'

test_expect_success 'multiple commits' '
	cd ssh-signed-repo &&
	echo more >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m second &&
	git log --oneline >actual &&
	test_line_count = 2 actual
'

test_expect_success 'cat-file shows tree and parent' '
	cd ssh-signed-repo &&
	git cat-file -p HEAD >actual &&
	grep "^tree " actual &&
	grep "^parent " actual &&
	grep "^author " actual
'

test_expect_success 'rev-list works' '
	cd ssh-signed-repo &&
	git rev-list HEAD >actual &&
	test_line_count = 2 actual
'

test_done
