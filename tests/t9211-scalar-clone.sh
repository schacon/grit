#!/bin/sh
#
# Tests for `scalar clone` subcommand.
#

test_description='test the `scalar clone` subcommand'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'set up repository to clone' '
	git init to-clone &&
	(
		cd to-clone &&
		test_commit first &&
		test_commit second &&
		test_commit third &&
		git switch -c topic &&
		test_commit on-topic
	)
'

test_expect_success 'creates content in enlistment root' '
	scalar clone "$(pwd)/to-clone" cloned &&
	test -d cloned/src/.git &&
	test -f cloned/src/.git/config
'

test_expect_success 'with spaces' '
	scalar clone "$(pwd)/to-clone" "cloned with spaces" &&
	test -d "cloned with spaces/src/.git"
'

test_expect_failure 'partial clone if supported by server' '
	false
'

test_expect_failure 'fall back on full clone if partial unsupported' '
	false
'

test_expect_failure 'initializes sparse-checkout by default' '
	false
'

test_expect_success '--full-clone does not create sparse-checkout' '
	scalar clone --full-clone "$(pwd)/to-clone" full-clone &&
	test -d full-clone/src/.git &&
	! test -f full-clone/src/.git/info/sparse-checkout
'

test_expect_success '--single-branch clones HEAD only' '
	scalar clone --single-branch "$(pwd)/to-clone" single-br &&
	test -d single-br/src/.git
'

test_expect_failure '--no-single-branch clones all branches' '
	false
'

test_expect_failure 'progress with tty' '
	false
'

test_expect_failure 'progress without tty' '
	false
'

test_expect_failure 'scalar clone warns when background maintenance fails' '
	false
'

test_expect_success 'scalar clone --no-maintenance' '
	scalar clone --no-maintenance "$(pwd)/to-clone" no-maint 2>stderr &&
	test -d no-maint/src/.git
'

test_expect_success '`scalar clone --no-src`' '
	scalar clone --no-src "$(pwd)/to-clone" no-src &&
	test -d no-src/.git &&
	! test -d no-src/src
'

test_done
