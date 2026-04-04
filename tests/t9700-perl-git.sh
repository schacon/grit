#!/bin/sh
# Ported from git/t/t9700-perl-git.sh
# perl interface (Git.pm)

test_description='perl interface (Git.pm)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'perl git module — not yet ported' '
	false
'

test_done
