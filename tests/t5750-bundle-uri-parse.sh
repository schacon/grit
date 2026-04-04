#!/bin/sh
# Ported from git/t/t5750-bundle-uri-parse.sh
# Test bundle-uri bundle_uri_parse_line()

test_description='Test bundle-uri bundle_uri_parse_line()'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'protocol (may require server) — not yet ported' '
	false
'

test_done
