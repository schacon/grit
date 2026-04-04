#!/bin/sh
# Ported from git/t/t5411-proc-receive-hook.sh
# Tests for proc-receive hook
# The upstream test is extremely complex with many sub-test-includes.
# Grit does not support the proc-receive hook, so all tests are expected failures.

test_description='proc-receive hook'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit initial
'

# grit does not support proc-receive hook
test_expect_failure 'standard git push without proc-receive' '
	git init --bare upstream.git &&
	git push ./upstream.git main &&
	git --git-dir=upstream.git rev-parse main >expect &&
	git rev-parse main >actual &&
	test_cmp expect actual &&
	# Setup proc-receive hook
	mkdir -p upstream.git/hooks &&
	write_script upstream.git/hooks/proc-receive <<-\EOF &&
	printf >&2 "# proc-receive hook\n"
	test-tool proc-receive
	EOF
	# Push to a special ref that should trigger proc-receive
	test_must_fail git push ./upstream.git HEAD:refs/for/main
'

test_done
