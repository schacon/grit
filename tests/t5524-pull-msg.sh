#!/bin/sh
# Ported from git/t/t5524-pull-msg.sh

test_description='git pull message generation'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	echo original >afile &&
	git add afile &&
	git commit -m initial &&
	git clone . cloned
'

test_expect_success 'pull fast-forward into cloned repo' '
	echo "second" >afile &&
	git add afile &&
	git commit -m "second commit" &&
	(
		cd cloned &&
		git pull &&
		git log -n 1 --format=%s >result &&
		grep "second commit" result
	)
'

test_done
