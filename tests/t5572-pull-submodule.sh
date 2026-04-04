#!/bin/sh
# Ported from git/t/t5572-pull-submodule.sh
# Tests pull with submodules

test_description='pull can handle submodules'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit initial
'

test_expect_success 'pull in basic repo works' '
	git clone . downstream &&
	test_commit upstream_change &&
	(
		cd downstream &&
		git pull
	) &&
	git -C downstream rev-parse HEAD >actual &&
	git rev-parse HEAD >expect &&
	test_cmp expect actual
'

# grit pull --rebase optional value parsing: use explicit syntax
test_expect_success 'pull --rebase works' '
	(
		cd downstream &&
		test_commit local_change &&
		git pull --rebase=true origin main
	)
'

# grit does not support --recurse-submodules for pull
test_expect_failure 'pull --recurse-submodules' '
	git init sub &&
	test_commit -C sub submod_commit &&
	git submodule add ./sub sub &&
	git commit -m "add submodule" &&
	git clone --recurse-submodules . with-sub &&
	test_commit -C sub another_submod_commit &&
	(
		cd with-sub &&
		git pull --recurse-submodules
	)
'

test_done
