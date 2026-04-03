#!/bin/sh
#
# Ported from git/t/t2017-checkout-orphan.sh (subset for switch --orphan)

test_description='git switch --orphan'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

# grit does not support checkout --orphan, but test switch --orphan instead
test_expect_success 'setup' '
	echo "Initial" >foo &&
	git add foo &&
	test_tick &&
	git commit -m "First Commit" &&
	echo "State 1" >>foo &&
	git add foo &&
	test_tick &&
	git commit -m "Second Commit"
'

test_expect_success 'switch --orphan creates a new orphan branch from HEAD' '
	git switch --orphan alpha &&
	test_must_fail git rev-parse --verify HEAD &&
	test "refs/heads/alpha" = "$(git symbolic-ref HEAD)"
'

test_expect_success 'checkout --orphan creates a new orphan branch' '
	git checkout master &&
	git checkout --orphan beta &&
	test_must_fail git rev-parse --verify HEAD &&
	test "refs/heads/beta" = "$(git symbolic-ref HEAD)"
'

test_expect_success 'checkout --orphan fails if branch already exists' '
	git checkout master &&
	test_must_fail git checkout --orphan master 2>err &&
	grep "already exists" err
'

test_done
