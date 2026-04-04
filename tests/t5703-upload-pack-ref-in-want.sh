#!/bin/sh
# Ported from git/t/t5703-upload-pack-ref-in-want.sh
# upload-pack ref-in-want

test_description='upload-pack ref-in-want'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'protocol (may require server) — not yet ported' '
	false
'

test_done
