#!/bin/sh

test_description='git for-each-repo builtin'

. ./test-lib.sh

test_expect_success 'setup' '
	git init one &&
	(cd one && git config user.name T && git config user.email t@t) &&
	git init two &&
	(cd two && git config user.name T && git config user.email t@t) &&
	git config --global run.key "$(pwd)/one" &&
	git config --global --add run.key "$(pwd)/two"
'

test_expect_success 'run based on configured value' '
	git config --global --replace-all run.key "$(pwd)/one" &&
	git for-each-repo --config=run.key -- commit --allow-empty -m "ran" &&
	(cd one && git log --max-count=1 --format=%s >message) &&
	grep ran one/message
'

test_expect_success 'do nothing on empty config' '
	git for-each-repo --config=bogus.config -- version
'

test_expect_success 'error on bad config keys' '
	test_expect_code 129 git for-each-repo --config=a &&
	test_expect_code 129 git for-each-repo --config=a.b.
'

test_done
