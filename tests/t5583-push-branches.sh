#!/bin/sh
# Ported from git/t/t5583-push-branches.sh
# check the consisitency of behavior of --all and --branches

test_description='check the consisitency of behavior of --all and --branches'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
