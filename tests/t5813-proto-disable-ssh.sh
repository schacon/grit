#!/bin/sh
# Ported from git/t/t5813-proto-disable-ssh.sh
# Tests for protocol.ssh.allow configuration
#
# Requires SSH transport and protocol.*.allow support. Stubbed.

test_description='protocol disabling for ssh:// transport'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_failure 'clone denied with protocol.ssh.allow=never' '
	test_must_fail git -c protocol.ssh.allow=never clone ssh://localhost/repo clone-denied 2>err &&
	grep -i "not allowed" err
'

test_expect_failure 'fetch denied with protocol.ssh.allow=never' '
	false
'

test_done
