#!/bin/sh
# Ported from git/t/t6030-bisect-porcelain.sh
# Tests git bisect functionality

test_description='Tests git bisect functionality'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

add_line_into_file()
{
    _line=$1
    _file=$2

    if [ -f "$_file" ]; then
        echo "$_line" >> $_file || return $?
        MSG="Add <$_line> into <$_file>."
    else
        echo "$_line" > $_file || return $?
        git add $_file || return $?
        MSG="Create file <$_file> with <$_line> inside."
    fi

    test_tick
    git add $_file &&
    git commit --quiet -m "$MSG"
}

HASH1=
HASH2=
HASH3=
HASH4=

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.email "test@example.com" &&
	git config user.name "Test User"
'

test_expect_success 'set up basic repo with 4 commits' '
	add_line_into_file "1: Hello World" hello &&
	HASH1=$(git rev-parse --verify HEAD) &&
	add_line_into_file "2: A new day for git" hello &&
	HASH2=$(git rev-parse --verify HEAD) &&
	add_line_into_file "3: Another new day for git" hello &&
	HASH3=$(git rev-parse --verify HEAD) &&
	add_line_into_file "4: Ciao for now" hello &&
	HASH4=$(git rev-parse --verify HEAD)
'

test_expect_success 'bisect start' '
	git bisect start
'

test_expect_success 'bisect bad' '
	git bisect bad $HASH4
'

test_expect_success 'bisect good' '
	git bisect good $HASH1
'

test_expect_success 'bisect reset' '
	git bisect reset
'

test_expect_success 'bisect start with one bad and good finds culprit' '
	git bisect start &&
	git bisect bad $HASH4 &&
	git bisect good $HASH1
'

test_expect_success 'bisect reset after finding culprit' '
	git bisect reset
'

test_expect_success 'bisect with 2 commits' '
	git bisect start &&
	git bisect bad $HASH2 &&
	git bisect good $HASH1 &&
	git bisect reset
'

test_expect_success 'bisect log works during bisect' '
	git bisect start &&
	git bisect bad $HASH4 &&
	git bisect good $HASH1 &&
	git bisect log >log_output &&
	test -s log_output &&
	git bisect reset
'

test_expect_success 'bisect reset returns to original branch' '
	git checkout main &&
	git bisect start &&
	git bisect bad $HASH4 &&
	git bisect good $HASH1 &&
	git bisect reset &&
	current=$(git symbolic-ref HEAD) &&
	test "refs/heads/main" = "$current"
'

test_expect_success 'bisect run finds the first bad commit' '
	git checkout main &&
	git bisect start &&
	git bisect bad $HASH4 &&
	git bisect good $HASH1 &&
	git bisect run test ! -f hello_extra 2>run_err &&
	git bisect reset
'

test_expect_success 'bisect run with a script finds culprit' '
	git checkout main &&
	git bisect start &&
	git bisect bad $HASH4 &&
	git bisect good $HASH2 &&
	git bisect run sh -c "test \$(git rev-list --count HEAD) -le 2" >run_out 2>&1 &&
	git bisect reset
'

test_expect_success 'bisect terms shows default terms' '
	git bisect start &&
	git bisect terms >terms_out &&
	grep "bad" terms_out &&
	grep "good" terms_out &&
	git bisect reset
'

test_expect_success 'bisect replay replays a log file' '
	git checkout main &&
	git bisect start &&
	git bisect bad $HASH4 &&
	git bisect good $HASH1 &&
	git bisect log >saved_log &&
	git bisect reset &&
	git bisect replay saved_log &&
	git bisect reset
'

test_done
