#!/bin/sh
# Ported from git/t/t5811-proto-disable-git.sh
# test disabling of git-over-tcp in clone/fetch

test_description='test disabling of git-over-tcp in clone/fetch'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'proto-disable — not yet ported' '
	false
'

test_done
