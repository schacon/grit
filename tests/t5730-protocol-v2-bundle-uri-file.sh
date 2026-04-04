#!/bin/sh
# Ported from git/t/t5730-protocol-v2-bundle-uri-file.sh
# Test bundle-uri with protocol v2 and 

test_description='Test bundle-uri with protocol v2 and '

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'protocol (may require server) — not yet ported' '
	false
'

test_done
