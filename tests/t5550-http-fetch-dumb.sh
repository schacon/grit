#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5550-http-fetch-dumb.sh
# test dumb fetching over http via static file

test_description='test dumb fetching over http via static file'
=======
#
# Upstream: t5550-http-fetch-dumb.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
#

test_description='test dumb fetching over http via static file (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
# --- HTTP dumb transport not yet available in grit ---

test_expect_failure 'setup repository' '
	false
'

test_expect_failure 'packfile without repository does not crash' '
	false
'

test_expect_failure 'create http-accessible bare repository with loose objects' '
	false
'

test_expect_failure 'clone http repository' '
	false
'

test_expect_failure 'list refs from outside any repository' '
	false
'

test_expect_failure 'fetch packed objects' '
	false
'

test_expect_failure 'fetch loose objects' '
	false
'

test_expect_failure 'did not use upload-pack service' '
>>>>>>> test/batch-EN
	false
'

test_done
