#!/bin/sh
# Ported from git/t/t5411-proc-receive-hook.sh
# Test proc-receive hook

test_description='Test proc-receive hook'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
