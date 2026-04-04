#!/bin/sh
# Ported from git/t/t5570-git-daemon.sh
# test fetching over git protocol

test_description='test fetching over git protocol'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
