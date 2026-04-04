#!/bin/sh
# Ported from git/t/t5410-receive-pack.sh
# Tests for git receive-pack

test_description='git receive-pack'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit base &&
	git clone -s --bare . fork &&
	git checkout -b public/branch main &&
	test_commit public &&
	git checkout -b private/branch main &&
	test_commit private
'

# The upstream tests for core.alternateRefsCommand and
# core.alternateRefsPrefixes require depacketize (a git test-tool)
# and pipe to receive-pack directly. Grit does not support these config
# options, so skip these tests entirely.

test_expect_success 'receive-pack basic connectivity via push' '
	git init --bare basic-dest.git &&
	git push ./basic-dest.git main &&
	commit=$(git rev-parse main) &&
	git --git-dir=basic-dest.git cat-file -t $commit >actual &&
	echo commit >expect &&
	test_cmp expect actual
'

test_expect_success 'receive-pack handles multiple branches' '
	git push ./basic-dest.git public/branch private/branch &&
	git --git-dir=basic-dest.git rev-parse public/branch >actual_pub &&
	git rev-parse public/branch >expect_pub &&
	test_cmp expect_pub actual_pub &&
	git --git-dir=basic-dest.git rev-parse private/branch >actual_priv &&
	git rev-parse private/branch >expect_priv &&
	test_cmp expect_priv actual_priv
'

test_expect_success 'receive-pack rejects non-fast-forward by default' '
	git checkout main &&
	git init --bare strict-dest.git &&
	git push ./strict-dest.git main &&
	test_commit advance1 &&
	git push ./strict-dest.git main &&
	git reset --hard HEAD^ &&
	test_commit diverged &&
	test_must_fail git push ./strict-dest.git main
'

test_expect_success 'receive-pack allows force push' '
	git push --force ./strict-dest.git main
'

test_done
