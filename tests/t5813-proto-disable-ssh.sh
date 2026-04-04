#!/bin/sh
# Ported from git/t/t5813-proto-disable-ssh.sh
# Tests for protocol.ssh.allow configuration

test_description='protocol disabling for ssh:// transport'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'clone denied with protocol.ssh.allow=never (ssh:// URL)' '
	test_must_fail git -c protocol.ssh.allow=never clone ssh://localhost/repo clone-denied-ssh 2>err &&
	grep -i "not allowed" err
'

test_expect_success 'clone denied with protocol.ssh.allow=never (host:path URL)' '
	test_must_fail git -c protocol.ssh.allow=never clone localhost:/repo clone-denied-scp 2>err &&
	grep -i "not allowed" err
'

test_done
