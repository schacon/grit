#!/bin/sh
#
# Upstream: t9815-git-p4-submit-fail.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 submit failure handling'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'conflict on one commit' '
	false
'

test_expect_failure 'conflict on second of two commits' '
	false
'

test_expect_failure 'conflict on first of two commits, skip' '
	false
'

test_expect_failure 'conflict on first of two commits, quit' '
	false
'

test_expect_failure 'conflict cli and config options' '
	false
'

test_expect_failure 'conflict on first of two commits, --conflict=skip' '
	false
'

test_expect_failure 'conflict on first of two commits, --conflict=quit' '
	false
'

test_expect_failure 'cleanup edit p4 populate' '
	false
'

test_expect_failure 'cleanup edit after submit fail' '
	false
'

test_expect_failure 'cleanup add after submit fail' '
	false
'

test_expect_failure 'cleanup delete after submit fail' '
	false
'

test_expect_failure 'cleanup copy after submit fail' '
	false
'

test_expect_failure 'cleanup rename after submit fail' '
	false
'

test_expect_failure 'cleanup edit after submit cancel' '
	false
'

test_expect_failure 'cleanup add after submit cancel' '
	false
'

test_expect_failure 'cleanup delete after submit cancel' '
	false
'

test_expect_failure 'cleanup copy after submit cancel' '
	false
'

test_expect_failure 'cleanup rename after submit cancel' '
	false
'

test_expect_failure 'cleanup chmod after submit cancel' '
	false
'

test_done
