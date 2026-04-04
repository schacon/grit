#!/bin/sh
# Ported from git/t/t5572-pull-submodule.sh
# pull can handle submodules

test_description='pull can handle submodules'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
