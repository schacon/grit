#!/bin/sh
#
# Upstream: t9802-git-p4-filetype.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 filetype tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'p4 client newlines, unix' '
	false
'

test_expect_failure 'p4 client newlines, win' '
	false
'

test_expect_failure 'ensure blobs store only lf newlines' '
	false
'

test_expect_failure 'gitattributes setting eol=lf produces lf newlines' '
	false
'

test_expect_failure 'gitattributes setting eol=crlf produces crlf newlines' '
	false
'

test_expect_failure 'crlf cleanup' '
	false
'

test_expect_failure 'utf-16 file create' '
	false
'

test_expect_failure 'utf-16 file test' '
	false
'

test_expect_failure 'keyword file create' '
	false
'

test_expect_failure 'keyword file test' '
	false
'

test_expect_failure 'ignore apple' '
	false
'

test_expect_failure 'create p4 symlink' '
	false
'

test_expect_failure 'ensure p4 symlink parsed correctly' '
	false
'

test_expect_failure 'empty symlink target' '
	false
'

test_expect_failure 'utf-8 with and without BOM in text file' '
	false
'

test_done
