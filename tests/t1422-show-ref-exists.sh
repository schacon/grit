#!/bin/sh
# Ported from git/t/t1422-show-ref-exists.sh (harness-compatible subset).

test_description='show-ref --exists'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

. "$TEST_DIRECTORY"/show-ref-exists-tests.sh

test_done
