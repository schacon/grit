#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t5541-http-push-smart.sh
# test smart pushing over http via http-backend

test_description='test smart pushing over http via http-backend'
=======
#
# Upstream: t5541-http-push-smart.sh
# Requires HTTP server (lib-httpd.sh) — stubbed as test_expect_failure.
# NOTE: grit already has t5541-remote-subcommands.sh (different test).
#

test_description='test smart pushing over http via http-backend (HTTP STUB)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
>>>>>>> test/batch-EN

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

<<<<<<< HEAD
test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'HTTP transport (requires httpd) — not yet ported' '
=======
# --- HTTP smart push not yet available in grit ---

test_expect_failure 'setup remote repository' '
	false
'

test_expect_failure 'clone remote repository' '
	false
'

test_expect_failure 'push to remote repository (standard)' '
	false
'

test_expect_failure 'used receive-pack service' '
	false
'

test_expect_failure 'push to remote repository (standard) with sending Accept-Language' '
	false
'

test_expect_failure 'push large request' '
	false
'

test_expect_failure 'push to password-protected repository' '
	false
'

test_expect_failure 'push --atomic to remote repository' '
>>>>>>> test/batch-EN
	false
'

test_done
