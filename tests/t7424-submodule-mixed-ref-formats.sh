#!/bin/sh
#
# Ported from git/t/t7424-submodule-mixed-ref-formats.sh
# Tests for submodules handling mixed ref storage formats
# Note: grit doesn't support reftable yet, so mark as expected failure
#

test_description='submodules handle mixed ref storage formats'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	"$REAL_GIT" init sub &&
	(
		cd sub &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo sub >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m initial
	) &&
	"$REAL_GIT" init super &&
	(
		cd super &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo super >file &&
		"$REAL_GIT" add file &&
		"$REAL_GIT" commit -m initial &&
		"$REAL_GIT" submodule add "$TRASH_DIRECTORY/sub" sub &&
		"$REAL_GIT" commit -m "add submodule"
	)
'

test_expect_success 'submodule status in files ref format' '
	cd super &&
	git submodule status >actual &&
	grep sub actual
'

test_done
