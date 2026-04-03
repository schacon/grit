#!/bin/sh

test_description='test trace2 facility (normal target)'

. ./test-lib.sh

# Turn off any inherited trace2 settings for this test.
sane_unset GIT_TRACE2 GIT_TRACE2_PERF GIT_TRACE2_EVENT
sane_unset GIT_TRACE2_BRIEF
sane_unset GIT_TRACE2_CONFIG_PARAMS

test_expect_success 'setup' '
	git init
'

test_expect_success 'GIT_TRACE2 produces output' '
	GIT_TRACE2="$(pwd)/trace.normal" git version >out &&
	test_path_is_file trace.normal &&
	test_file_not_empty trace.normal
'

test_expect_success 'GIT_TRACE2 contains version event' '
	GIT_TRACE2="$(pwd)/trace2.normal" git version >out &&
	grep "version" trace2.normal
'

test_expect_success 'GIT_TRACE2 contains exit event' '
	GIT_TRACE2="$(pwd)/trace3.normal" git version >out &&
	grep "exit" trace3.normal
'

test_done
