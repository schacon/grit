#!/bin/sh

test_description='test trace2 facility (perf target)'

. ./test-lib.sh

sane_unset GIT_TRACE2 GIT_TRACE2_PERF GIT_TRACE2_EVENT

test_expect_success 'setup' '
	git init
'

test_expect_failure 'GIT_TRACE2_PERF produces output' '
	GIT_TRACE2_PERF="$(pwd)/trace.perf" git version >out &&
	test_path_is_file trace.perf &&
	test_file_not_empty trace.perf
'

test_done
