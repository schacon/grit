#!/bin/sh

test_description='simple command server'

. ./test-lib.sh

# These tests require test-tool simple-ipc which is not available in grit.

test_expect_success 'setup' '
	git init
'

test_expect_failure 'simple IPC tests (requires test-tool)' '
	test-tool simple-ipc SUPPORTS_SIMPLE_IPC
'

test_done
