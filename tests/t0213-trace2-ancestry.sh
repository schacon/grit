#!/bin/sh

test_description='test trace2 cmd_ancestry event'

. ./test-lib.sh

sane_unset GIT_TRACE2 GIT_TRACE2_PERF GIT_TRACE2_EVENT

test_expect_success 'setup' '
	git init
'

test_expect_failure 'trace2 ancestry event (requires test-tool and trace2)' '
	GIT_TRACE2="$(pwd)/trace.ancestry" git version >out &&
	grep "cmd_ancestry" trace.ancestry
'

test_done
