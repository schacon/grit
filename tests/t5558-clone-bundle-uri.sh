#!/bin/sh
# Ported from git/t/t5558-clone-bundle-uri.sh
# test fetching bundles with --bundle-uri

test_description='test fetching bundles with --bundle-uri'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
