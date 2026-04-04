#!/bin/sh
# Ported from git/t/t7426-submodule-get-default-remote.sh
# git submodule--helper get-default-remote

test_description='git submodule--helper get-default-remote'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
	false
'

test_done
