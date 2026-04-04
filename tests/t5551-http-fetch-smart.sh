#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5551-http-fetch-smart.sh
# test smart fetching over http via http-backend ($HTTP_PROTO)

test_description='test smart fetching over http via http-backend ($HTTP_PROTO)'
=======
#
# Upstream: t5551-http-fetch-smart.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test smart fetching over http via http-backend (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
# --- HTTP smart transport not yet available in grit ---

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'create http-accessible bare repository' '
	false
'

test_expect_failure 'clone http repository' '
	false
'

test_expect_failure 'fetch changes via http' '
	false
'

test_expect_failure 'used upload-pack service' '
	false
'

test_expect_failure 'follow redirects (301)' '
	false
'

test_expect_failure 'follow redirects (302)' '
	false
'

test_expect_failure 'clone from password-protected repository' '
>>>>>>> test/batch-EN
	false
'

test_done
