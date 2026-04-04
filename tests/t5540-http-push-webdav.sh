#!/bin/sh
# Ported from git/t/t5540-http-push-webdav.sh
# test WebDAV http-push

test_description='test WebDAV http-push'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
	false
'

test_done
