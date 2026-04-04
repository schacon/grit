#!/bin/sh

test_description='test git rev-parse --parseopt'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'parseopt basic usage' '
	cat >optionspec <<-\EOF &&
	some-command [options] <args>...

	some-command does foo and bar!
	--
	h,help    show the help
	foo       some nifty option --foo
	bar=      some cool option --bar with an argument
	EOF
	git rev-parse --parseopt -- --foo <optionspec >actual &&
	grep "foo" actual
'

test_expect_success 'parseopt with --help' '
	cat >optionspec <<-\EOF &&
	some-command [options] <args>...
	--
	h,help    show the help
	EOF
	test_expect_code 129 git rev-parse --parseopt -- --help <optionspec >output 2>&1
'

test_done
