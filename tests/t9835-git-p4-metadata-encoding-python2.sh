#!/bin/sh
#
# Upstream: t9835-git-p4-metadata-encoding-python2.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 metadata encoding

This test checks that the import process handles inconsistent text
encoding in p4 metadata (author names, commit messages, etc) without
failing, and produces maximally sane output in git.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'clone non-utf8 repo with strict encoding' '
	false
'

test_expect_failure 'check utf-8 contents with passthrough strategy' '
	false
'

test_expect_failure 'check latin-1 contents corrupted in git with passthrough strategy' '
	false
'

test_expect_failure 'check utf-8 contents with fallback strategy' '
	false
'

test_expect_failure 'check latin-1 contents with fallback strategy' '
	false
'

test_expect_failure 'check cp-1252 contents with fallback strategy' '
	false
'

test_expect_failure 'check cp850 contents parsed with correct fallback' '
	false
'

test_expect_failure 'check cp850-only contents escaped when cp1252 is fallback' '
	false
'

test_expect_failure 'check cp-1252 contents on later sync after clone with fallback strategy' '
	false
'

test_expect_failure 'passthrough (latin-1 contents corrupted in git) is the default with python2' '
	false
'

test_done
