#!/bin/sh
#
# Upstream: t5541-http-push-smart.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test smart pushing over http via http-backend'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

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

test_expect_failure 'push already up-to-date' '
	false
'

test_expect_failure 'create and delete remote branch' '
	false
'

test_expect_failure 'setup rejected update hook' '
	false
'

test_expect_failure 'rejected update prints status' '
	false
'

test_expect_failure 'push fails for non-fast-forward refs unmatched by remote helper' '
	false
'

test_expect_failure 'push fails for non-fast-forward refs unmatched by remote helper: remote output' '
	false
'

test_expect_failure 'push fails for non-fast-forward refs unmatched by remote helper: our output' '
	false
'

test_expect_failure 'push (chunked)' '
	false
'

test_expect_failure 'push --atomic also prevents branch creation, reports collateral' '
	false
'

test_expect_failure 'push --atomic fails on server-side errors' '
	false
'

test_expect_failure 'push --all can push to empty repo' '
	false
'

test_expect_failure 'push --mirror can push to empty repo' '
	false
'

test_expect_failure 'push --all to repo with alternates' '
	false
'

test_expect_failure 'push --mirror to repo with alternates' '
	false
'

test_expect_failure 'push shows progress when stderr is a tty' '
	false
'

test_expect_failure 'push --quiet silences status and progress' '
	false
'

test_expect_failure 'push --no-progress silences progress but not status' '
	false
'

test_expect_failure 'push --progress shows progress to non-tty' '
	false
'

test_expect_failure 'http push gives sane defaults to reflog' '
	false
'

test_expect_failure 'http push respects GIT_COMMITTER_* in reflog' '
	false
'

test_expect_failure 'push over smart http with auth' '
	false
'

test_expect_failure 'push to auth-only-for-push repo' '
	false
'

test_expect_failure 'create repo without http.receivepack set' '
	false
'

test_expect_failure 'clone via half-auth-complete does not need password' '
	false
'

test_expect_failure 'push into half-auth-complete requires password' '
	false
'

test_expect_failure 'push 2000 tags over http' '
	false
'

test_expect_failure 'push with post-receive to inspect certificate' '
	false
'

test_expect_failure 'push status output scrubs password' '
	false
'

test_expect_failure 'clone/fetch scrubs password from reflogs' '
	false
'

test_expect_failure 'Non-ASCII branch name can be used with --force-with-lease' '
	false
'

test_expect_failure 'colorize errors/hints' '
	false
'

test_expect_failure 'report error server does not provide ref status' '
	false
'

test_done
