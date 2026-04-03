#!/bin/sh

test_description='test trace2 facility (event target)'

. ./test-lib.sh

sane_unset GIT_TRACE2 GIT_TRACE2_PERF GIT_TRACE2_EVENT

test_expect_success 'setup' '
	git init
'

test_expect_success 'GIT_TRACE2_EVENT produces output' '
	GIT_TRACE2_EVENT="$(pwd)/trace.event" git version >out &&
	test_path_is_file trace.event &&
	test_file_not_empty trace.event
'

test_done
