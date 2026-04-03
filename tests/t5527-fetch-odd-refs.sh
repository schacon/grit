#!/bin/sh
# Ported from git/t/t5527-fetch-odd-refs.sh

test_description='test fetching of oddly-named refs'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup repo with odd suffix ref' '
	git init -q &&
	echo content >file &&
	git add . &&
	git commit -m one &&
	git update-ref refs/for/refs/heads/main HEAD &&
	echo content >>file &&
	git commit -a -m two &&
	echo content >>file &&
	git commit -a -m three
'

test_expect_success 'clone gets the right main branch content' '
	git clone . suffix &&
	(
		cd suffix &&
		echo three >expect &&
		git log -n 1 --format=%s main >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'fetch from repo with odd ref names' '
	git clone . fetcher &&
	git update-ref refs/oddname/test HEAD &&
	(
		cd fetcher &&
		git fetch origin &&
		git rev-parse origin/main
	)
'

test_done
