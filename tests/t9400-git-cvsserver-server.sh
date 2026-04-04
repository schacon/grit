#!/bin/sh
#
# Upstream: t9400-git-cvsserver-server.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git-cvsserver access

tests read access to a git repository with the
cvs CLI client via git-cvsserver server'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- CVS not available in grit ---

test_expect_failure 'setup' '
	false
'

test_expect_failure 'basic checkout' '
	false
'

test_expect_failure 'pserver authentication' '
	false
'

test_expect_failure 'pserver authentication failure (non-anonymous user)' '
	false
'

test_expect_failure 'pserver authentication success (non-anonymous user with password)' '
	false
'

test_expect_failure 'pserver authentication (login)' '
	false
'

test_expect_failure 'pserver authentication failure (login/non-anonymous user)' '
	false
'

test_expect_failure 'req_Root failure (relative pathname)' '
	false
'

test_expect_failure 'req_Root failure (conflicting roots)' '
	false
'

test_expect_failure 'req_Root (strict paths)' '
	false
'

test_expect_failure 'req_Root failure (strict-paths)' '
	false
'

test_expect_failure 'req_Root (w/o strict-paths)' '
	false
'

test_expect_failure 'req_Root failure (w/o strict-paths)' '
	false
'

test_expect_failure 'req_Root (base-path)' '
	false
'

test_expect_failure 'req_Root failure (base-path)' '
	false
'

test_expect_failure 'req_Root (export-all)' '
	false
'

test_expect_failure 'req_Root failure (export-all w/o directory list)' '
	false
'

test_expect_failure 'req_Root (everything together)' '
	false
'

test_expect_failure 'gitcvs.enabled = false' '
	false
'

test_expect_failure 'gitcvs.ext.enabled = true' '
	false
'

test_expect_failure 'gitcvs.ext.enabled = false' '
	false
'

test_expect_failure 'gitcvs.dbname' '
	false
'

test_expect_failure 'gitcvs.ext.dbname' '
	false
'

test_expect_failure 'cvs update (create new file)' '
	false
'

test_expect_failure 'cvs update (update existing file)' '
	false
'

test_expect_failure 'cvs update (subdirectories)' '
	false
'

test_expect_failure 'cvs update (delete file)' '
	false
'

test_expect_failure 'cvs update (re-add deleted file)' '
	false
'

test_expect_failure 'cvs update (merge)' '
	false
'

test_expect_failure 'cvs update (conflict merge)' '
	false
'

test_expect_failure 'cvs update (-C)' '
	false
'

test_expect_failure 'cvs update (merge no-op)' '
	false
'

test_expect_failure 'cvs update (-p)' '
	false
'

test_expect_failure 'cvs update (module list supports packed refs)' '
	false
'

test_expect_failure 'cvs status' '
	false
'

test_expect_failure 'cvs status (nonrecursive)' '
	false
'

test_expect_failure 'cvs status (no subdirs in header)' '
	false
'

test_expect_failure 'cvs co -c (shows module database)' '
	false
'

test_expect_failure 'cvs log' '
	false
'

test_expect_failure 'cvs annotate' '
	false
'

test_expect_failure 'create remote-cvs helper' '
	false
'

test_expect_failure 'cvs server does not run with vanilla git-shell' '
	false
'

test_expect_failure 'configure git shell to run cvs server' '
	false
'

test_expect_failure 'cvs server can run with recommended config' '
	false
'

test_expect_failure 'cvs update w/o -d doesn'\''t create subdir (TODO)' '
	false
'

test_done
