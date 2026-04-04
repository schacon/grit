#!/bin/sh
test_description='fetch/push functionality using the HTTP protocol'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh
. "$TEST_DIRECTORY"/lib-httpd.sh
start_httpd

SERVER="$HTTPD_DOCUMENT_ROOT_PATH/server"
URI="$HTTPD_URL/smart/server"

grep_wrote () {
	grep 'write_pack_file/wrote.*"value":"'$1'"' $2
}

setup_client_and_server () {
	rm -rf client "$SERVER" &&
	git init client && test_commit -C client first_commit && test_commit -C client second_commit &&
	git init "$SERVER" && git --git-dir="$SERVER/.git" config http.receivepack true &&
	test_commit -C "$SERVER" unrelated_commit &&
	git -C client push "$URI" first_commit:refs/remotes/origin/first_commit &&
	git -C "$SERVER" config receive.hideRefs refs/remotes/origin/first_commit
}

test_expect_success 'push without negotiation' '
	setup_client_and_server &&
	GIT_TRACE2_EVENT="$(pwd)/event" git -C client -c protocol.version=2 \
		push "$URI" refs/heads/main:refs/remotes/origin/main &&
	test_when_finished "rm -f event" &&
	grep_wrote 6 event
'
test_expect_success 'push with negotiation' '
	setup_client_and_server &&
	GIT_TRACE2_EVENT="$(pwd)/event" git -C client -c protocol.version=2 -c push.negotiate=1 \
		push "$URI" refs/heads/main:refs/remotes/origin/main &&
	test_when_finished "rm -f event" &&
	grep_wrote 3 event
'
test_expect_success 'push with negotiation proceeds anyway even if negotiation fails' '
	setup_client_and_server &&
	GIT_TEST_PROTOCOL_VERSION=0 GIT_TRACE2_EVENT="$(pwd)/event" git -C client -c push.negotiate=1 \
		push "$URI" refs/heads/main:refs/remotes/origin/main 2>err &&
	test_when_finished "rm -f event" &&
	grep_wrote 6 event &&
	grep "warning:.*negotiate-only.*requires protocol v2" err &&
	grep "warning: push negotiation failed" err
'
test_done
