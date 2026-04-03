#!/bin/sh
# Ported from git/t/t5519-push-alternates.sh

test_description='push to a repository that borrows from elsewhere'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success setup '
	git init -q &&
	mkdir alice-pub &&
	(
		cd alice-pub &&
		git init --bare
	) &&
	mkdir alice-work &&
	(
		cd alice-work &&
		git init &&
		>file &&
		git add . &&
		git commit -m initial &&
		git remote add origin ../alice-pub &&
		git push origin main
	) &&

	# Project Bob is a fork of project Alice
	git clone alice-pub bob-work &&
	git init --bare bob-pub &&
	(
		cd bob-work &&
		git remote add bob ../bob-pub &&
		git push bob main
	)
'

test_expect_success 'alice works and pushes' '
	(
		cd alice-work &&
		echo more >file &&
		git commit -a -m second &&
		git push origin main
	)
'

test_expect_success 'bob fetches from alice, works and pushes' '
	(
		cd bob-work &&
		git fetch origin &&
		git merge origin/main &&
		echo more bob >file &&
		git commit -a -m third &&
		git push bob main
	)
'

test_done
