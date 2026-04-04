#!/bin/sh
#
# Upstream: t5560-http-backend-noserver.sh
# Tests git-http-backend directly as CGI (no HTTP server needed).
#
# Note: This test exercises git's own http-backend CGI program.
# We use real git for http-backend since grit doesn't implement it yet,
# but grit for all other git operations (init, push, config, etc.).
#

test_description='test git-http-backend-noserver'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

HTTPD_DOCUMENT_ROOT_PATH="$TRASH_DIRECTORY"

# We need real git's http-backend binary
REAL_GIT_HTTP_BACKEND="$(command -v git-http-backend 2>/dev/null)"
if test -z "$REAL_GIT_HTTP_BACKEND"
then
	# Try finding it via git --exec-path
	_exec_path="$(/usr/bin/git --exec-path 2>/dev/null)"
	if test -n "$_exec_path" && test -x "$_exec_path/git-http-backend"
	then
		REAL_GIT_HTTP_BACKEND="$_exec_path/git-http-backend"
	fi
fi

if test -z "$REAL_GIT_HTTP_BACKEND" || ! test -x "$REAL_GIT_HTTP_BACKEND"
then
	skip_all='git-http-backend not found'
	test_done
fi

run_backend() {
	echo "$2" |
	QUERY_STRING="${1#*[?]}" \
	PATH_TRANSLATED="$HTTPD_DOCUMENT_ROOT_PATH/${1%%[?]*}" \
	"$REAL_GIT_HTTP_BACKEND" >act.out 2>act.err
}

GET() {
	REQUEST_METHOD="GET" && export REQUEST_METHOD &&
	run_backend "/repo.git/$1" &&
	sane_unset REQUEST_METHOD &&
	if ! grep "Status" act.out >act
	then
		printf "Status: 200 OK\r\n" >act
	fi
	printf "Status: $2\r\n" >exp &&
	test_cmp exp act
}

POST() {
	REQUEST_METHOD="POST" && export REQUEST_METHOD &&
	CONTENT_TYPE="application/x-$1-request" && export CONTENT_TYPE &&
	run_backend "/repo.git/$1" "$2" &&
	sane_unset REQUEST_METHOD &&
	sane_unset CONTENT_TYPE &&
	if ! grep "Status" act.out >act
	then
		printf "Status: 200 OK\r\n" >act
	fi
	printf "Status: $3\r\n" >exp &&
	test_cmp exp act
}

. "$TEST_DIRECTORY"/t556x_common

expect_aliased() {
	REQUEST_METHOD="GET" && export REQUEST_METHOD &&
	if test $1 = 0; then
		run_backend "$2"
	else
		run_backend "$2" &&
		echo "fatal: '$2': aliased" >exp.err &&
		test_cmp exp.err act.err
	fi
	unset REQUEST_METHOD
}

test_expect_success 'http-backend blocks bad PATH_INFO' '
	config http.getanyfile true &&

	expect_aliased 0 /repo.git/HEAD &&

	expect_aliased 1 /repo.git/../HEAD &&
	expect_aliased 1 /../etc/passwd &&
	expect_aliased 1 ../etc/passwd &&
	expect_aliased 1 /etc//passwd &&
	expect_aliased 1 /etc/./passwd &&
	expect_aliased 1 //domain/data.txt
'

test_done
