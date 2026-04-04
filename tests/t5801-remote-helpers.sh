#!/bin/sh
# Ported from git/t/t5801-remote-helpers.sh
# Test remote-helper import and export commands

test_description='Test remote-helper import and export commands'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'proto-disable — not yet ported' '
	false
'

test_done
