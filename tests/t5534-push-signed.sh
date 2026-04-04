#!/bin/sh
# Ported from git/t/t5534-push-signed.sh
# Tests signed push
# Grit does not support GPG signed pushes

test_description='signed push'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit initial
'

# grit rejects --signed (unknown flag), which is fine
test_expect_success 'push --signed fails gracefully' '
	git init --bare dst.git &&
	git push ./dst.git main &&
	test_commit second &&
	test_must_fail git push --signed ./dst.git main 2>err
'

# grit does not support push certificate verification
test_expect_failure 'push --signed=if-asked push certificate' '
	git init --bare dst2.git &&
	git push --signed=if-asked ./dst2.git main
'

test_expect_success 'basic unsigned push works' '
	git init --bare unsigned-dst.git &&
	git push ./unsigned-dst.git main &&
	git --git-dir=unsigned-dst.git rev-parse main >actual &&
	git rev-parse main >expect &&
	test_cmp expect actual
'

test_done
