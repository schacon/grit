#!/bin/sh
#
# Upstream: t5550-http-fetch-dumb.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test dumb fetching over http via static file'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

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

test_expect_failure 'list detached HEAD from outside any repository' '
	false
'

test_expect_failure 'create password-protected repository' '
	false
'

test_expect_failure 'create empty remote repository' '
	false
'

test_expect_failure 'empty dumb HTTP repository falls back to SHA1' '
	false
'

test_expect_failure 'cloning password-protected repository can fail' '
	false
'

test_expect_failure 'using credentials from netrc to clone successfully' '
	false
'

test_expect_failure 'netrc unauthorized credentials (prompt after 401)' '
	false
'

test_expect_failure 'netrc authorized but forbidden credentials (fail on 403)' '
	false
'

test_expect_failure 'http auth can use user/pass in URL' '
	false
'

test_expect_failure 'http auth can use just user in URL' '
	false
'

test_expect_failure 'http auth can request both user and pass' '
	false
'

test_expect_failure 'http auth respects credential helper config' '
	false
'

test_expect_failure 'http auth can get username from config' '
	false
'

test_expect_failure 'configured username does not override URL' '
	false
'

test_expect_failure 'set up repo with http submodules' '
	false
'

test_expect_failure 'cmdline credential config passes to submodule via clone' '
	false
'

test_expect_failure 'cmdline credential config passes submodule via fetch' '
	false
'

test_expect_failure 'cmdline credential config passes submodule update' '
	false
'

test_expect_failure 'fetch changes via http' '
	false
'

test_expect_failure 'fetch changes via manual http-fetch' '
	false
'

test_expect_failure 'manual http-fetch without -a works just as well' '
	false
'

test_expect_failure 'http remote detects correct HEAD' '
	false
'

test_expect_failure 'fetch packed objects' '
	false
'

test_expect_failure 'http-fetch --packfile' '
	false
'

test_expect_failure 'fetch notices corrupt pack' '
	false
'

test_expect_failure 'http-fetch --packfile with corrupt pack' '
	false
'

test_expect_failure 'fetch notices corrupt idx' '
	false
'

test_expect_failure 'fetch can handle previously-fetched .idx files' '
	false
'

test_expect_failure 'did not use upload-pack service' '
	false
'

test_expect_failure 'git client shows text/plain errors' '
	false
'

test_expect_failure 'git client does not show html errors' '
	false
'

test_expect_failure 'git client shows text/plain with a charset' '
	false
'

test_expect_failure 'http error messages are reencoded' '
	false
'

test_expect_failure 'reencoding is robust to whitespace oddities' '
	false
'

test_expect_failure 'git client sends Accept-Language based on LANGUAGE' '
	false
'

test_expect_failure 'git client sends Accept-Language correctly with unordinary LANGUAGE' '
	false
'

test_expect_failure 'git client sends Accept-Language with many preferred languages' '
	false
'

test_expect_failure 'git client send an empty Accept-Language' '
	false
'

test_expect_failure 'remote-http complains cleanly about malformed urls' '
	false
'

test_expect_failure 'remote-http complains cleanly about empty scheme' '
	false
'

test_expect_failure 'redirects can be forbidden/allowed' '
	false
'

test_expect_failure 'redirects are reported to stderr' '
	false
'

test_expect_failure 'non-initial redirects can be forbidden' '
	false
'

test_expect_failure 'http.followRedirects defaults to "initial"' '
	false
'

test_expect_failure 'set up evil alternates scheme' '
	false
'

test_expect_failure 'http-alternates is a non-initial redirect' '
	false
'

test_expect_failure 'http-alternates cannot point at funny protocols' '
	false
'

test_expect_failure 'http-alternates triggers not-from-user protocol check' '
	false
'

test_expect_failure 'can redirect through non-"info/refs?service=git-upload-pack" URL' '
	false
'

test_expect_failure 'print HTTP error when any intermediate redirect throws error' '
	false
'

test_expect_failure 'fetching via http alternates works' '
	false
'

test_expect_failure 'dumb http can fetch index v1' '
	false
'

test_done
