#!/bin/sh
# Ported from git/t/t5813-proto-disable-ssh.sh
# test disabling of git-over-ssh in clone/fetch

test_description='test disabling of git-over-ssh in clone/fetch'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'proto-disable — not yet ported' '
	false
'

test_done
