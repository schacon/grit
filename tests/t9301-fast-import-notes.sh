#!/bin/sh
#
# Upstream: t9301-fast-import-notes.sh
# Requires fast-import — ported as test_expect_failure stubs.
#

test_description='test git fast-import of notes objects'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- fast-import not available in grit ---

test_expect_failure 'set up main branch' '
	false
'

test_expect_failure 'add notes with simple M command' '
	false
'

test_expect_failure 'add notes with simple N command' '
	false
'

test_expect_failure 'update existing notes with N command' '
	false
'

test_expect_failure 'add concatenation notes with M command' '
	false
'

test_expect_failure 'verify that deleteall also removes notes' '
	false
'

test_expect_failure 'verify that later N commands override earlier M commands' '
	false
'

test_expect_failure 'add lots of commits and notes' '
	false
'

test_expect_failure 'verify that lots of notes trigger a fanout scheme' '
	false
'

test_expect_failure 'verify that importing a notes tree respects the fanout scheme' '
	false
'

test_expect_failure 'verify that non-notes are untouched by a fanout change' '
	false
'

test_expect_failure 'change a few existing notes' '
	false
'

test_expect_failure 'verify that changing notes respect existing fanout' '
	false
'

test_expect_failure 'remove lots of notes' '
	false
'

test_expect_failure 'verify that removing notes trigger fanout consolidation' '
	false
'

test_expect_failure 'verify that non-notes are untouched by a fanout change' '
	false
'

test_expect_failure 'add notes to $num_commits commits in each of $num_notes_refs refs' '
	false
'

test_done
