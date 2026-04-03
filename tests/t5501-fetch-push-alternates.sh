#!/bin/sh

test_description='fetch/push involving alternates'
GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success setup '
	git init &&
	(
		git init original &&
		cd original &&
		i=0 &&
		while test $i -le 5
		do
			echo "$i" >count &&
			git add count &&
			git commit -m "$i" || exit
			i=$(($i + 1))
		done
	) &&
	git clone original one &&
	(
		cd one &&
		echo Z >count &&
		git add count &&
		git commit -m Z
	)
'

test_expect_success 'push to a sibling clone' '
	git clone original receiver &&
	(
		cd one &&
		git push ../receiver main:refs/heads/from-one
	)
'

test_expect_success 'fetch from a sibling clone' '
	git clone original fetcher &&
	(
		cd fetcher &&
		git fetch ../one main:refs/heads/from-one
	)
'

test_done
