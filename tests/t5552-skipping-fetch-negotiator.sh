#!/bin/sh
# Ported from git/t/t5552-skipping-fetch-negotiator.sh
# test skipping fetch negotiator

test_description='test skipping fetch negotiator'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
