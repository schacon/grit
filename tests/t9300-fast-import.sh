#!/bin/sh
#
# Upstream: t9300-fast-import.sh
# Requires fast-import — ported as test_expect_failure stubs.
#

test_description='test git fast-import utility'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- fast-import not available in grit ---

test_expect_failure 'empty stream succeeds' '
	false
'

test_expect_failure 'truncated stream complains' '
	false
'

test_expect_failure 'A: create pack from stdin' '
	false
'

test_expect_failure 'A: verify pack' '
	false
'

test_expect_failure 'A: verify commit' '
	false
'

test_expect_failure 'A: verify tree' '
	false
'

test_expect_failure 'A: verify file2' '
	false
'

test_expect_failure 'A: verify file3' '
	false
'

test_expect_failure 'A: verify file4' '
	false
'

test_expect_failure 'A: verify tag/series-A' '
	false
'

test_expect_failure 'A: verify tag/series-A-blob' '
	false
'

test_expect_failure 'A: verify tag deletion is successful' '
	false
'

test_expect_failure 'A: verify marks output' '
	false
'

test_expect_failure 'A: verify marks import' '
	false
'

test_expect_failure 'A: tag blob by sha1' '
	false
'

test_expect_failure 'A: verify marks import does not crash' '
	false
'

test_expect_failure 'A: verify pack' '
	false
'

test_expect_failure 'A: verify diff' '
	false
'

test_expect_failure 'A: export marks with large values' '
	false
'

test_expect_failure 'B: fail on invalid blob sha1' '
	false
'

test_expect_failure 'B: accept branch name "TEMP_TAG"' '
	false
'

test_expect_failure 'B: accept empty committer' '
	false
'

test_expect_failure 'B: reject invalid timezone' '
	false
'

test_expect_failure 'B: accept invalid timezone with raw-permissive' '
	false
'

test_expect_failure 'B: accept and fixup committer with no name' '
	false
'

test_expect_failure 'B: fail on invalid committer (1)' '
	false
'

test_expect_failure 'B: fail on invalid committer (2)' '
	false
'

test_expect_failure 'B: fail on invalid committer (3)' '
	false
'

test_expect_failure 'B: fail on invalid committer (4)' '
	false
'

test_expect_failure 'B: fail on invalid committer (5)' '
	false
'

test_expect_failure 'B: fail on invalid file path of ..' '
	false
'

test_expect_failure 'B: fail on invalid file path of .' '
	false
'

test_expect_failure 'B: fail on invalid file path of C:' '
	false
'

test_expect_failure 'B: fail on invalid file path of .git' '
	false
'

test_expect_failure 'B: fail on invalid file path of .gitmodules' '
	false
'

test_expect_failure 'C: incremental import create pack from stdin' '
	false
'

test_expect_failure 'C: verify pack' '
	false
'

test_expect_failure 'C: validate reuse existing blob' '
	false
'

test_expect_failure 'C: verify commit' '
	false
'

test_expect_failure 'C: validate rename result' '
	false
'

test_expect_failure 'D: inline data in commit' '
	false
'

test_expect_failure 'D: verify pack' '
	false
'

test_expect_failure 'D: validate new files added' '
	false
'

test_expect_failure 'D: verify file5' '
	false
'

test_expect_failure 'D: verify file6' '
	false
'

test_expect_failure 'E: rfc2822 date, --date-format=raw' '
	false
'

test_expect_failure 'E: rfc2822 date, --date-format=rfc2822' '
	false
'

test_expect_failure 'E: verify pack' '
	false
'

test_expect_failure 'E: verify commit' '
	false
'

test_expect_failure 'F: non-fast-forward update skips' '
	false
'

test_expect_failure 'F: verify pack' '
	false
'

test_expect_failure 'F: verify other commit' '
	false
'

test_expect_failure 'G: non-fast-forward update forced' '
	false
'

test_expect_failure 'G: verify pack' '
	false
'

test_expect_failure 'G: branch changed, but logged' '
	false
'

test_expect_failure 'H: deletall, add 1' '
	false
'

test_expect_failure 'H: verify pack' '
	false
'

test_expect_failure 'H: validate old files removed, new files added' '
	false
'

test_expect_failure 'H: verify file' '
	false
'

test_expect_failure 'I: export-pack-edges' '
	false
'

test_expect_failure 'I: verify edge list' '
	false
'

test_expect_failure 'J: reset existing branch creates empty commit' '
	false
'

test_expect_failure 'J: branch has 1 commit, empty tree' '
	false
'

test_expect_failure 'J: tag must fail on empty branch' '
	false
'

test_expect_failure 'K: reinit branch with from' '
	false
'

test_expect_failure 'K: verify K^1 = branch^1' '
	false
'

test_expect_failure 'L: verify internal tree sorting' '
	false
'

test_expect_failure 'L: nested tree copy does not corrupt deltas' '
	false
'

test_expect_failure 'M: rename file in same subdirectory' '
	false
'

test_expect_failure 'M: rename file to new subdirectory' '
	false
'

test_expect_failure 'M: rename subdirectory to new subdirectory' '
	false
'

test_expect_failure 'N: copy file in same subdirectory' '
	false
'

test_expect_failure 'N: copy then modify subdirectory' '
	false
'

test_expect_failure 'N: copy dirty subdirectory' '
	false
'

test_expect_failure 'N: copy directory by id' '
	false
'

test_expect_failure 'N: read and copy directory' '
	false
'

test_expect_failure 'N: empty directory reads as missing' '
	false
'

test_expect_failure 'N: delete directory by copying' '
	false
'

test_expect_failure 'N: modify copied tree' '
	false
'

test_expect_failure 'N: reject foo/ syntax' '
	false
'

test_expect_failure 'N: reject foo/ syntax in copy source' '
	false
'

test_expect_failure 'N: reject foo/ syntax in rename source' '
	false
'

test_expect_failure 'N: reject foo/ syntax in ls argument' '
	false
'

test_expect_failure 'O: comments are all skipped' '
	false
'

test_expect_failure 'O: blank lines not necessary after data commands' '
	false
'

test_expect_failure 'O: repack before next test' '
	false
'

test_expect_failure 'O: blank lines not necessary after other commands' '
	false
'

test_expect_failure 'O: progress outputs as requested by input' '
	false
'

test_expect_failure 'P: superproject & submodule mix' '
	false
'

test_expect_failure 'P: verbatim SHA gitlinks' '
	false
'

test_expect_failure 'P: fail on inline gitlink' '
	false
'

test_expect_failure 'P: fail on blob mark in gitlink' '
	false
'

test_expect_failure 'Q: commit notes' '
	false
'

test_expect_failure 'Q: verify pack' '
	false
'

test_expect_failure 'Q: verify first commit' '
	false
'

test_expect_failure 'Q: verify second commit' '
	false
'

test_expect_failure 'Q: verify third commit' '
	false
'

test_expect_failure 'Q: verify first notes commit' '
	false
'

test_expect_failure 'Q: verify first notes tree' '
	false
'

test_expect_failure 'Q: verify first note for first commit' '
	false
'

test_expect_failure 'Q: verify first note for second commit' '
	false
'

test_expect_failure 'Q: verify first note for third commit' '
	false
'

test_expect_failure 'Q: verify second notes commit' '
	false
'

test_expect_failure 'Q: verify second notes tree' '
	false
'

test_expect_failure 'Q: verify second note for first commit' '
	false
'

test_expect_failure 'Q: verify first note for second commit' '
	false
'

test_expect_failure 'Q: verify first note for third commit' '
	false
'

test_expect_failure 'Q: verify third notes commit' '
	false
'

test_expect_failure 'Q: verify third notes tree' '
	false
'

test_expect_failure 'Q: verify third note for first commit' '
	false
'

test_expect_failure 'Q: verify fourth notes commit' '
	false
'

test_expect_failure 'Q: verify fourth notes tree' '
	false
'

test_expect_failure 'Q: verify second note for second commit' '
	false
'

test_expect_failure 'Q: deny note on empty branch' '
	false
'

test_expect_failure 'R: abort on unsupported feature' '
	false
'

test_expect_failure 'R: supported feature is accepted' '
	false
'

test_expect_failure 'R: abort on receiving feature after data command' '
	false
'

test_expect_failure 'R: import-marks features forbidden by default' '
	false
'

test_expect_failure 'R: only one import-marks feature allowed per stream' '
	false
'

test_expect_failure 'R: export-marks feature forbidden by default' '
	false
'

test_expect_failure 'R: export-marks feature results in a marks file being created' '
	false
'

test_expect_failure 'R: export-marks options can be overridden by commandline options' '
	false
'

test_expect_failure 'R: catch typo in marks file name' '
	false
'

test_expect_failure 'R: import and output marks can be the same file' '
	false
'

test_expect_failure 'R: --import-marks=foo --output-marks=foo to create foo fails' '
	false
'

test_expect_failure 'R: --import-marks-if-exists' '
	false
'

test_expect_failure 'R: feature import-marks-if-exists' '
	false
'

test_expect_failure 'R: import to output marks works without any content' '
	false
'

test_expect_failure 'R: import marks prefers commandline marks file over the stream' '
	false
'

test_expect_failure 'R: multiple --import-marks= should be honoured' '
	false
'

test_expect_failure 'R: feature relative-marks should be honoured' '
	false
'

test_expect_failure 'R: feature no-relative-marks should be honoured' '
	false
'

test_expect_failure 'R: feature ls supported' '
	false
'

test_expect_failure 'R: feature cat-blob supported' '
	false
'

test_expect_failure 'R: cat-blob-fd must be a nonnegative integer' '
	false
'

test_expect_failure 'R: print old blob' '
	false
'

test_expect_failure 'R: in-stream cat-blob-fd not respected' '
	false
'

test_expect_failure 'R: print mark for new blob' '
	false
'

test_expect_failure 'R: print new blob' '
	false
'

test_expect_failure 'R: print new blob by sha1' '
	false
'

test_expect_failure 'setup: big file' '
	false
'

test_expect_failure 'R: print two blobs to stdout' '
	false
'

test_expect_failure 'R: copy using cat-file' '
	false
'

test_expect_failure 'R: print blob mid-commit' '
	false
'

test_expect_failure 'R: print staged blob within commit' '
	false
'

test_expect_failure 'R: quiet option results in no stats being output' '
	false
'

test_expect_failure 'R: feature done means terminating "done" is mandatory' '
	false
'

test_expect_failure 'R: terminating "done" with trailing gibberish is ok' '
	false
'

test_expect_failure 'R: terminating "done" within commit' '
	false
'

test_expect_failure 'R: die on unknown option' '
	false
'

test_expect_failure 'R: unknown commandline options are rejected' '
	false
'

test_expect_failure 'R: die on invalid option argument' '
	false
'

test_expect_failure 'R: ignore non-git options' '
	false
'

test_expect_failure 'R: corrupt lines do not mess marks file' '
	false
'

test_expect_failure 'R: blob bigger than threshold' '
	false
'

test_expect_failure 'R: verify created pack' '
	false
'

test_expect_failure 'R: verify written objects' '
	false
'

test_expect_failure 'R: blob appears only once' '
	false
'

test_expect_failure 'S: initialize for S tests' '
	false
'

test_expect_failure 'S: filemodify with garbage after mark must fail' '
	false
'

test_expect_failure 'S: filemodify with garbage after inline must fail' '
	false
'

test_expect_failure 'S: filemodify with garbage after sha1 must fail' '
	false
'

test_expect_failure 'S: notemodify with garbage after mark dataref must fail' '
	false
'

test_expect_failure 'S: notemodify with garbage after inline dataref must fail' '
	false
'

test_expect_failure 'S: notemodify with garbage after sha1 dataref must fail' '
	false
'

test_expect_failure 'S: notemodify with garbage after mark commit-ish must fail' '
	false
'

test_expect_failure 'S: from with garbage after mark must fail' '
	false
'

test_expect_failure 'S: merge with garbage after mark must fail' '
	false
'

test_expect_failure 'S: tag with garbage after mark must fail' '
	false
'

test_expect_failure 'S: cat-blob with garbage after mark must fail' '
	false
'

test_expect_failure 'S: ls with garbage after mark must fail' '
	false
'

test_expect_failure 'S: ls with garbage after sha1 must fail' '
	false
'

test_expect_failure 'T: delete branch' '
	false
'

test_expect_failure 'T: empty reset doesnt delete branch' '
	false
'

test_expect_failure 'U: initialize for U tests' '
	false
'

test_expect_failure 'U: filedelete file succeeds' '
	false
'

test_expect_failure 'U: validate file delete result' '
	false
'

test_expect_failure 'U: filedelete directory succeeds' '
	false
'

test_expect_failure 'U: validate directory delete result' '
	false
'

test_expect_failure 'V: checkpoint helper does not get stuck with extra output' '
	false
'

test_expect_failure 'V: checkpoint updates refs after reset' '
	false
'

test_expect_failure 'V: checkpoint updates refs and marks after commit' '
	false
'

test_expect_failure 'V: checkpoint updates refs and marks after commit (no new objects)' '
	false
'

test_expect_failure 'V: checkpoint updates tags after tag' '
	false
'

test_expect_failure 'W: get-mark & empty orphan commit with no newlines' '
	false
'

test_expect_failure 'W: get-mark & empty orphan commit with one newline' '
	false
'

test_expect_failure 'W: get-mark & empty orphan commit with ugly second newline' '
	false
'

test_expect_failure 'W: get-mark & empty orphan commit with erroneous third newline' '
	false
'

test_expect_failure 'X: handling encoding' '
	false
'

test_expect_failure 'X: replace ref that becomes useless is removed' '
	false
'

test_expect_failure 'Y: setup' '
	false
'

test_expect_failure 'Y: rewrite submodules' '
	false
'

test_expect_failure 'M: rename root ($root) to subdirectory' '
	false
'

test_expect_failure 'N: copy root ($root) by tree hash' '
	false
'

test_expect_failure 'N: copy root ($root) by path' '
	false
'

test_expect_failure 'N: copy to root ($root) by id and modify' '
	false
'

test_expect_failure 'N: extract subtree to the root ($root)' '
	false
'

test_expect_failure 'N: modify subtree, extract it to the root ($root), and modify again' '
	false
'

test_expect_failure 'S: paths at EOL with $test must work' '
	false
'

test_expect_failure 'S: paths before space with $test must work' '
	false
'

test_expect_failure 'S: $change with $what must fail' '
	false
'

test_expect_failure 'T: ls root ($root) tree' '
	false
'

test_expect_failure 'U: filedelete root ($root) succeeds' '
	false
'

test_expect_failure 'U: validate root ($root) delete result' '
	false
'

test_done
