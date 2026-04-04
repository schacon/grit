#!/bin/sh
# Ported from git/t/t5571-pre-push-hook.sh

test_description='check pre-push hooks'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_failure 'setup (grit: pre-push hook receives resolved ref instead of HEAD)' '
	git init &&
	test_hook pre-push <<-\EOF &&
	cat >actual
	EOF

	git config push.default upstream &&
	git init --bare repo1 &&
	git remote add parent1 repo1 &&
	test_commit one &&
	cat >expect <<-EOF &&
	HEAD $(git rev-parse HEAD) refs/heads/foreign $ZERO_OID
	EOF

	test_when_finished "rm actual" &&
	git push parent1 HEAD:foreign &&
	test_cmp expect actual
'

COMMIT1="$(git rev-parse HEAD)"
export COMMIT1

test_expect_failure 'push with failing hook (grit: pre-push hook ref format differs)' '
	test_hook pre-push <<-\EOF &&
	cat >actual &&
	exit 1
	EOF

	test_commit two &&
	cat >expect <<-EOF &&
	HEAD $(git rev-parse HEAD) refs/heads/main $ZERO_OID
	EOF

	test_when_finished "rm actual" &&
	test_must_fail git push parent1 HEAD &&
	test_cmp expect actual
'

test_expect_failure '--no-verify bypasses hook (grit: --no-verify not supported)' '
	git push --no-verify parent1 HEAD &&
	test_path_is_missing actual
'

COMMIT2="$(git rev-parse HEAD)"
export COMMIT2

test_expect_success 'push with hook' '
	test_hook --setup pre-push <<-\EOF &&
	echo "$1" >actual
	echo "$2" >>actual
	cat >>actual
	EOF

	cat >expect <<-EOF &&
	parent1
	repo1
	refs/heads/main $COMMIT2 refs/heads/foreign $COMMIT1
	EOF

	git push parent1 main:foreign &&
	test_cmp expect actual
'

test_expect_failure 'add a branch (grit: tracking branch checkout syntax)' '
	git checkout -b other parent1/foreign &&
	test_commit three
'

COMMIT3="$(git rev-parse HEAD)"
export COMMIT3

test_expect_failure 'push to default (grit: push.default upstream not supported)' '
	cat >expect <<-EOF &&
	parent1
	repo1
	refs/heads/other $COMMIT3 refs/heads/foreign $COMMIT2
	EOF
	git push &&
	test_cmp expect actual
'

test_expect_failure 'push non-branches (grit: push tag as source ref)' '
	cat >expect <<-EOF &&
	parent1
	repo1
	refs/tags/one $COMMIT1 refs/tags/tag1 $ZERO_OID
	HEAD~ $COMMIT2 refs/heads/prev $ZERO_OID
	EOF

	git push parent1 one:tag1 HEAD~:refs/heads/prev &&
	test_cmp expect actual
'

test_expect_failure 'push delete (grit: push delete ref format)' '
	cat >expect <<-EOF &&
	parent1
	repo1
	(delete) $ZERO_OID refs/heads/prev $COMMIT2
	EOF

	git push parent1 :prev &&
	test_cmp expect actual
'

test_expect_failure 'push to URL (grit: push to path as URL)' '
	cat >expect <<-EOF &&
	repo1
	repo1
	HEAD $COMMIT3 refs/heads/other $ZERO_OID
	EOF

	git push repo1 HEAD &&
	test_cmp expect actual
'

test_expect_success 'set up many-ref tests' '
	{
		nr=1000 &&
		while test $nr -lt 2000
		do
			nr=$(( $nr + 1 )) &&
			echo "create refs/heads/b/$nr $COMMIT3" || return 1
		done
	} | git update-ref --stdin
'

test_expect_failure 'sigpipe does not cause pre-push hook failure (grit: glob refspec expansion)' '
	test_hook --clobber pre-push <<-\EOF &&
	exit 0
	EOF
	git push parent1 "refs/heads/b/*:refs/heads/b/*"
'

test_done
