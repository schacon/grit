#!/bin/sh
# Ported from git/t/t9836-git-p4-metadata-encoding-python3.sh
# git p4 metadata encoding

test_description='git p4 metadata encoding'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
