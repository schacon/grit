#!/bin/sh
# Ported from git/t/t9351-fast-export-anonymize.sh
# basic tests for fast-export --anonymize

test_description='basic tests for fast-export --anonymize'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'fast-import/export — not yet ported' '
	false
'

test_done
