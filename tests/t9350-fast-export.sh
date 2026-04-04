#!/bin/sh
#
# Upstream: t9350-fast-export.sh
# Requires fast-export — ported as test_expect_failure stubs.
#

test_description='git fast-export'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- fast-export not available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'fast-export | fast-import' '
	false
'

test_expect_failure 'fast-export ^muss^{commit} muss' '
	false
'

test_expect_failure 'fast-export --mark-tags ^muss^{commit} muss' '
	false
'

test_expect_failure 'fast-export main~2..main' '
	false
'

test_expect_failure 'fast-export --reference-excluded-parents main~2..main' '
	false
'

test_expect_failure 'fast-export --show-original-ids' '
	false
'

test_expect_failure 'fast-export --show-original-ids | git fast-import' '
	false
'

test_expect_failure 'reencoding iso-8859-7' '
	false
'

test_expect_failure 'aborting on iso-8859-7' '
	false
'

test_expect_failure 'preserving iso-8859-7' '
	false
'

test_expect_failure 'encoding preserved if reencoding fails' '
	false
'

test_expect_failure 'import/export-marks' '
	false
'

test_expect_failure 'set up faked signed tag' '
	false
'

test_expect_failure 'signed-tags=abort' '
	false
'

test_expect_failure 'signed-tags=verbatim' '
	false
'

test_expect_failure 'signed-tags=warn-verbatim' '
	false
'

test_expect_failure 'signed-tags=warn' '
	false
'

test_expect_failure 'signed-tags=strip' '
	false
'

test_expect_failure 'signed-tags=warn-strip' '
	false
'

test_expect_failure 'setup X.509 signed tag' '
	false
'

test_expect_failure 'signed-tags=verbatim with X.509' '
	false
'

test_expect_failure 'signed-tags=strip with X.509' '
	false
'

test_expect_failure 'setup SSH signed tag' '
	false
'

test_expect_failure 'signed-tags=verbatim with SSH' '
	false
'

test_expect_failure 'signed-tags=strip with SSH' '
	false
'

test_expect_failure 'set up signed commit' '
	false
'

test_expect_failure 'signed-commits default is same as strip' '
	false
'

test_expect_failure 'signed-commits=abort' '
	false
'

test_expect_failure 'signed-commits=verbatim' '
	false
'

test_expect_failure 'signed-commits=warn-verbatim' '
	false
'

test_expect_failure 'signed-commits=strip' '
	false
'

test_expect_failure 'signed-commits=warn-strip' '
	false
'

test_expect_failure 'setup X.509 signed commit' '
	false
'

test_expect_failure 'round-trip X.509 signed commit' '
	false
'

test_expect_failure 'setup SSH signed commit' '
	false
'

test_expect_failure 'round-trip SSH signed commit' '
	false
'

test_expect_failure 'setup submodule' '
	false
'

test_expect_failure 'submodule fast-export | fast-import' '
	false
'

test_expect_failure 'setup copies' '
	false
'

test_expect_failure 'fast-export -C -C | fast-import' '
	false
'

test_expect_failure 'fast-export | fast-import when main is tagged' '
	false
'

test_expect_failure 'cope with tagger-less tags' '
	false
'

test_expect_failure 'setup for limiting exports by PATH' '
	false
'

test_expect_failure 'dropping tag of filtered out object' '
	false
'

test_expect_failure 'rewriting tag of filtered out object' '
	false
'

test_expect_failure 'rewrite tag predating pathspecs to nothing' '
	false
'

test_expect_failure 'no exact-ref revisions included' '
	false
'

test_expect_failure 'path limiting with import-marks does not lose unmodified files' '
	false
'

test_expect_failure 'path limiting works' '
	false
'

test_expect_failure 'avoid corrupt stream with non-existent mark' '
	false
'

test_expect_failure 'full-tree re-shows unmodified files' '
	false
'

test_expect_failure 'set-up a few more tags for tag export tests' '
	false
'

test_expect_failure 'tree_tag' '
	false
'

test_expect_failure 'tree_tag-obj' '
	false
'

test_expect_failure 'tag-obj_tag' '
	false
'

test_expect_failure 'tag-obj_tag-obj' '
	false
'

test_expect_failure 'handling tags of blobs' '
	false
'

test_expect_failure 'handling nested tags' '
	false
'

test_expect_failure 'directory becomes symlink' '
	false
'

test_expect_failure 'fast-export quotes pathnames' '
	false
'

test_expect_failure 'test bidirectionality' '
	false
'

test_expect_failure 'avoid uninteresting refs' '
	false
'

test_expect_failure 'refs are updated even if no commits need to be exported' '
	false
'

test_expect_failure 'use refspec' '
	false
'

test_expect_failure 'delete ref because entire history excluded' '
	false
'

test_expect_failure 'delete refspec' '
	false
'

test_expect_failure 'when using -C, do not declare copy when source of copy is also modified' '
	false
'

test_expect_failure 'merge commit gets exported with --import-marks' '
	false
'

test_expect_failure 'fast-export --first-parent outputs all revisions output by revision walk' '
	false
'

test_expect_failure 'fast-export handles --end-of-options' '
	false
'

test_expect_failure 'setup a commit with dual signatures on its SHA-1 and SHA-256 formats' '
	false
'

test_expect_failure 'export and import of doubly signed commit' '
	false
'

test_done
