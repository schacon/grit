#!/bin/sh
# Ported from git/t/t9825-git-p4-handle-utf16-without-bom.sh
# git p4 handling of UTF-16 files without BOM

test_description='git p4 handling of UTF-16 files without BOM'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'git-p4 (requires Perforce) — not yet ported' '
	false
'

test_done
