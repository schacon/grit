#!/bin/sh
# Ported from git/t/t5521-pull-options.sh

test_description='pull options'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	mkdir parent &&
	(cd parent && git init &&
	 echo one >file && git add file &&
	 git commit -m one)
'

test_expect_success 'git pull -q from clone' '
	git clone parent clonedq &&
	(cd parent && echo two >file && git commit -a -m two) &&
	(cd clonedq &&
	 git pull -q >out 2>err &&
	 test_must_be_empty out)
'

test_expect_success 'git pull --rebase from clone' '
	git clone parent clonedrb &&
	(cd parent && echo three >file && git commit -a -m three) &&
	(cd clonedrb &&
	 git pull --rebase >out 2>err)
'

test_expect_success 'git pull --ff-only from clone' '
	git clone parent clonedff &&
	(cd parent && echo four >file && git commit -a -m four) &&
	(cd clonedff &&
	 git pull --ff-only >out 2>err)
'

test_expect_success 'git pull --ff-only fails on non-ff' '
	(cd parent && echo five >file && git commit -a -m five) &&
	(cd clonedff &&
	 echo local >localfile && git add localfile && git commit -m local &&
	 test_must_fail git pull --ff-only 2>err)
'

test_done
