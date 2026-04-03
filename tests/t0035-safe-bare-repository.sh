#!/bin/sh

test_description='verify safe.bareRepository checks'

. ./test-lib.sh

test_expect_success 'setup a bare repo' '
	git init --bare bare-repo
'

test_expect_success 'safe.bareRepository unset allows access' '
	git -C bare-repo rev-parse --git-dir
'

test_expect_success 'safe.bareRepository=all allows access' '
	git config --global safe.bareRepository all &&
	git -C bare-repo rev-parse --git-dir
'

test_expect_success 'safe.bareRepository=explicit rejects implicit bare repo' '
	git config --global safe.bareRepository explicit &&
	test_must_fail git -C bare-repo rev-parse --git-dir 2>err &&
	grep "cannot use bare repository" err
'

test_done
