#!/bin/sh
# Ported from git/t/t9001-send-email.sh
# git send-email

test_description='git send-email'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
