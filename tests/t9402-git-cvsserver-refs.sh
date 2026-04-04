#!/bin/sh
#
# Upstream: t9402-git-cvsserver-refs.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git-cvsserver and git refspecs

tests ability for git-cvsserver to switch between and compare
tags, branches and other git refspecs'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- CVS not available in grit ---

test_expect_failure 'setup v1, b1' '
	false
'

test_expect_failure 'cvs co v1' '
	false
'

test_expect_failure 'cvs co b1' '
	false
'

test_expect_failure 'cvs co b1 [cvswork3]' '
	false
'

test_expect_failure 'edit cvswork3 and save diff' '
	false
'

test_expect_failure 'setup v1.2 on b1' '
	false
'

test_expect_failure 'cvs -f up (on b1 adir)' '
	false
'

test_expect_failure 'cvs up (on b1 /)' '
	false
'

test_expect_failure 'cvs up (on b1 /) (again; check CVS/Tag files)' '
	false
'

test_expect_failure 'cvs up -r v1' '
	false
'

test_expect_failure 'cvs up' '
	false
'

test_expect_failure 'cvs up (again; check CVS/Tag files)' '
	false
'

test_expect_failure 'setup simple b2' '
	false
'

test_expect_failure 'cvs co b2 [into cvswork2]' '
	false
'

test_expect_failure 'root dir edit [cvswork2]' '
	false
'

test_expect_failure 'root dir rm file [cvswork2]' '
	false
'

test_expect_failure 'subdir edit/add/rm files [cvswork2]' '
	false
'

test_expect_failure 'validate result of edits [cvswork2]' '
	false
'

test_expect_failure 'validate basic diffs saved during above cvswork2 edits' '
	false
'

test_expect_failure 'validate v1.2 diff saved during last cvswork2 edit' '
	false
'

test_expect_failure 'validate v1.2 v1 diff saved during last cvswork2 edit' '
	false
'

test_expect_failure 'cvs up [cvswork2]' '
	false
'

test_expect_failure 'cvs up -r b2 [back to cvswork]' '
	false
'

test_expect_failure 'cvs up -r b1' '
	false
'

test_expect_failure 'cvs up -A' '
	false
'

test_expect_failure 'cvs up (check CVS/Tag files)' '
	false
'

test_expect_failure 'cvs up -r heads/b1' '
	false
'

test_expect_failure 'cvs up -r heads_-s-b2 (cvsserver escape mechanism)' '
	false
'

test_expect_failure 'cvs up -r $(git rev-parse v1)' '
	false
'

test_expect_failure 'cvs diff -r v1 -u' '
	false
'

test_expect_failure 'cvs diff -N -r v2 -u' '
	false
'

test_expect_failure 'cvs diff -N -r v2 -r v1.2' '
	false
'

test_expect_failure 'apply early [cvswork3] diff to b3' '
	false
'

test_expect_failure 'check [cvswork3] diff' '
	false
'

test_expect_failure 'merge early [cvswork3] b3 with b1' '
	false
'

test_expect_failure 'cvs up dirty [cvswork3]' '
	false
'

test_expect_failure 'cvs commit [cvswork3]' '
	false
'

test_done
