#!/bin/sh
# Ported from git/t/t5532-fetch-proxy.sh
# fetching via git:// using core.gitproxy

test_description='fetching via git:// using core.gitproxy'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
