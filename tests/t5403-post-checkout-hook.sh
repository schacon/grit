#!/bin/sh
# Ported from git/t/t5403-post-checkout-hook.sh
# Simplified: tests checkout functionality (hooks not yet supported)

test_description='Test checkout functionality'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	echo one >one.t &&
	git add one.t &&
	git commit -m one &&
	git tag one &&
	echo two >two.t &&
	git add two.t &&
	git commit -m two &&
	git tag two &&
	echo three >three.t &&
	git add three.t &&
	git commit -m three &&
	git tag three
'

test_expect_success 'checkout same branch is a no-op' '
	git checkout main &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse three)"
'

test_expect_success 'checkout -b creates new branch' '
	git checkout -b new1 &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse three)"
'

test_expect_success 'checkout to different commit changes HEAD' '
	old=$(git rev-parse HEAD) &&
	git checkout two &&
	new=$(git rev-parse HEAD) &&
	test "$old" != "$new" &&
	test "$(git rev-parse HEAD)" = "$(git rev-parse two)"
'

test_done
