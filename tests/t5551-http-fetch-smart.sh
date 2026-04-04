#!/bin/sh
#
# Upstream: t5551-http-fetch-smart.sh
# Requires HTTP transport — ported as test_expect_failure stubs.
#

test_description='test smart fetching over http via http-backend ($HTTP_PROTO)'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- HTTP transport not available in grit ---

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

test_expect_failure 'redirects re-root further requests' '
	false
'

test_expect_failure 're-rooting dies on insane schemes' '
	false
'

test_expect_failure 'clone from password-protected repository' '
	false
'

test_expect_failure 'credential.interactive=false skips askpass' '
	false
'

test_expect_failure 'clone from auth-only-for-push repository' '
	false
'

test_expect_failure 'clone from auth-only-for-objects repository' '
	false
'

test_expect_failure 'no-op half-auth fetch does not require a password' '
	false
'

test_expect_failure 'redirects send auth to new location' '
	false
'

test_expect_failure 'GIT_TRACE_CURL redacts auth details' '
	false
'

test_expect_failure 'GIT_CURL_VERBOSE redacts auth details' '
	false
'

test_expect_failure 'GIT_TRACE_CURL does not redact auth details if GIT_TRACE_REDACT=0' '
	false
'

test_expect_failure 'disable dumb http on server' '
	false
'

test_expect_failure 'GIT_SMART_HTTP can disable smart http' '
	false
'

test_expect_failure 'invalid Content-Type rejected' '
	false
'

test_expect_failure 'create namespaced refs' '
	false
'

test_expect_failure 'smart clone respects namespace' '
	false
'

test_expect_failure 'dumb clone via http-backend respects namespace' '
	false
'

test_expect_failure 'cookies stored in http.cookiefile when http.savecookies set' '
	false
'

test_expect_failure 'transfer.hiderefs works over smart-http' '
	false
'

test_expect_failure 'create 2,000 tags in the repo' '
	false
'

test_expect_failure 'large fetch-pack requests can be sent using chunked encoding' '
	false
'

test_expect_failure 'test allowreachablesha1inwant' '
	false
'

test_expect_failure 'test allowreachablesha1inwant with unreachable' '
	false
'

test_expect_failure 'test allowanysha1inwant with unreachable' '
	false
'

test_expect_failure 'http can handle enormous ref negotiation' '
	false
'

test_expect_failure 'custom http headers' '
	false
'

test_expect_failure 'using fetch command in remote-curl updates refs' '
	false
'

test_expect_failure 'fetch by SHA-1 without tag following' '
	false
'

test_expect_failure 'cookies are redacted by default' '
	false
'

test_expect_failure 'empty values of cookies are also redacted' '
	false
'

test_expect_failure 'GIT_TRACE_REDACT=0 disables cookie redaction' '
	false
'

test_expect_failure 'GIT_TRACE_CURL_NO_DATA prevents data from being traced' '
	false
'

test_expect_failure 'server-side error detected' '
	false
'

test_expect_failure 'http auth remembers successful credentials' '
	false
'

test_expect_failure 'http auth forgets bogus credentials' '
	false
'

test_expect_failure 'client falls back from v2 to v0 to match server' '
	false
'

test_expect_failure 'create empty http-accessible SHA-256 repository' '
	false
'

test_expect_failure 'clone empty SHA-256 repository with protocol v2' '
	false
'

test_expect_failure 'clone empty SHA-256 repository with protocol v0' '
	false
'

test_expect_failure 'passing hostname resolution information works' '
	false
'

test_expect_failure 'clone warns or fails when using username:password' '
	false
'

test_expect_failure 'clone does not detect username:password when it is https://username@domain:port/' '
	false
'

test_expect_failure 'fetch warns or fails when using username:password' '
	false
'

test_expect_failure 'push warns or fails when using username:password' '
	false
'

test_expect_failure 'no empty path components' '
	false
'

test_expect_failure 'tag following always works over v0 http' '
	false
'

test_done
