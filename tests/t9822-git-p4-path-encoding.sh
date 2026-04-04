#!/bin/sh
# Ported from git/t/t9822-git-p4-path-encoding.sh
# Clone repositories with non ASCII paths

test_description='Clone repositories with non ASCII paths'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
