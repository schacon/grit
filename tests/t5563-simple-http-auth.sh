#!/bin/sh
#
# Upstream: t5563-simple-http-auth.sh
# Tests HTTP auth header and credential helper interop.
# Requires Apache CGIPassAuth which our test-httpd doesn't support.
#

test_description='test http auth header and credential helper interop'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh
. "$TEST_DIRECTORY"/lib-httpd.sh

enable_cgipassauth
if ! test_have_prereq CGIPASSAUTH
then
	skip_all="no CGIPassAuth support (test-httpd does not support custom auth)"
	test_done
fi
start_httpd

test_expect_success 'setup_credential_helper' '
	mkdir "$TRASH_DIRECTORY/bin" &&
	PATH=$PATH:"$TRASH_DIRECTORY/bin" &&
	export PATH &&

	CREDENTIAL_HELPER="$TRASH_DIRECTORY/bin/git-credential-test-helper" &&
	write_script "$CREDENTIAL_HELPER" <<-\EOF
	cmd=$1
	teefile=$cmd-query-temp.cred
	catfile=$cmd-reply.cred
	sed -n -e "/^$/q" -e "p" >>$teefile
	state=$(sed -ne "s/^state\[\]=helper://p" "$teefile")
	if test -z "$state"
	then
		mv "$teefile" "$cmd-query.cred"
	else
		mv "$teefile" "$cmd-query-$state.cred"
		catfile="$cmd-reply-$state.cred"
	fi
	if test "$cmd" = "get"
	then
		cat $catfile
	fi
	EOF
'

set_credential_reply () {
	local suffix="$(test -n "$2" && echo "-$2")"
	cat >"$TRASH_DIRECTORY/$1-reply$suffix.cred"
}

expect_credential_query () {
	local suffix="$(test -n "$2" && echo "-$2")"
	cat >"$TRASH_DIRECTORY/$1-expect$suffix.cred" &&
	test_cmp "$TRASH_DIRECTORY/$1-expect$suffix.cred" \
		 "$TRASH_DIRECTORY/$1-query$suffix.cred"
}

per_test_cleanup () {
	rm -f *.cred &&
	rm -f "$HTTPD_ROOT_PATH"/custom-auth.valid \
	      "$HTTPD_ROOT_PATH"/custom-auth.challenge
}

test_expect_success 'setup repository' '
	test_commit foo &&
	git init --bare "$HTTPD_DOCUMENT_ROOT_PATH/repo.git" &&
	git push --mirror "$HTTPD_DOCUMENT_ROOT_PATH/repo.git"
'

test_expect_success 'access using basic auth' '
	test_when_finished "per_test_cleanup" &&

	set_credential_reply get <<-EOF &&
	username=alice
	password=secret-passwd
	EOF

	cat >"$HTTPD_ROOT_PATH/custom-auth.valid" <<-EOF &&
	id=1 creds=Basic YWxpY2U6c2VjcmV0LXBhc3N3ZA==
	EOF

	cat >"$HTTPD_ROOT_PATH/custom-auth.challenge" <<-EOF &&
	id=1 status=200
	id=default response=WWW-Authenticate: Basic realm="example.com"
	EOF

	test_config_global credential.helper test-helper &&
	git ls-remote "$HTTPD_URL/custom_auth/repo.git" &&

	expect_credential_query get <<-EOF &&
	capability[]=authtype
	capability[]=state
	protocol=http
	host=$HTTPD_DEST
	wwwauth[]=Basic realm="example.com"
	EOF

	expect_credential_query store <<-EOF
	protocol=http
	host=$HTTPD_DEST
	username=alice
	password=secret-passwd
	EOF
'

test_expect_success 'access using basic auth via authtype' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using basic auth invalid credentials' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using basic proactive auth' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using auto proactive auth with basic default' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using auto proactive auth with authtype from credential helper' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using basic auth with extra challenges' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using basic auth mixed-case wwwauth header name' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using basic auth with wwwauth header continuations' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using basic auth with wwwauth header empty continuations' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using basic auth with wwwauth header mixed continuations' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using bearer auth' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using bearer auth with invalid credentials' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'clone with bearer auth and probe_rpc' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_expect_success 'access using three-legged auth' '
	test_when_finished "per_test_cleanup" &&
	: # requires CGIPassAuth
'

test_done
