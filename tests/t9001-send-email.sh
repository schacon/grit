#!/bin/sh
#
# Upstream: t9001-send-email.sh
# Tests for 'git send-email' functionality.
#

test_description='git send-email'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'prepare reference tree' '
	git init send-email-repo &&
	cd send-email-repo &&
	echo "1A quick brown fox jumps over the" >file &&
	echo "lazy dog" >>file &&
	git add file &&
	GIT_AUTHOR_NAME="A" git commit -a -m "Initial."
'

test_expect_success 'Setup helper tool' '
	cd send-email-repo &&
	write_script fake.sendmail <<-\EOF &&
	output=1
	while test -f commandline$output
	do
		output=$(($output+1))
	done
	for a
	do
		echo "!$a!"
	done >commandline$output
	cat >"msgtxt$output"
	EOF
	git add fake.sendmail &&
	GIT_AUTHOR_NAME="A" git commit -a -m "Second."
'

test_expect_success 'Extract patches' '
	cd send-email-repo &&
	patches=$(git format-patch -s --cc="One <one@example.com>" --cc=two@example.com -n HEAD^1) &&
	test -n "$patches" &&
	test -f "$patches"
'

test_expect_success 'Send patches' '
	cd send-email-repo &&
	patches=$(git format-patch -n HEAD^1) &&
	git send-email \
		--suppress-cc=sob \
		--from="Example <nobody@example.com>" \
		--to=nobody@example.com \
		--smtp-server="$(pwd)/fake.sendmail" \
		--confirm=never \
		$patches 2>errors &&
	test -f msgtxt1
'

test_expect_success 'Verify commandline' '
	cd send-email-repo &&
	grep "!nobody@example.com!" commandline1
'

test_expect_success 'Send patches with --envelope-sender' '
	cd send-email-repo &&
	rm -f commandline* msgtxt* &&
	patches=$(git format-patch -n HEAD^1) &&
	git send-email \
		--from="Example <nobody@example.com>" \
		--to=nobody@example.com \
		--envelope-sender="sender@example.com" \
		--smtp-server="$(pwd)/fake.sendmail" \
		--confirm=never \
		$patches 2>errors &&
	grep "!-f!" commandline1 &&
	grep "!sender@example.com!" commandline1
'

test_expect_success 'setup expect for cc trailer' '
	cd send-email-repo &&
	rm -f commandline* msgtxt* &&
	echo "!one@example.com!" >expected-cc &&
	echo "!nobody@example.com!" >>expected-cc
'

test_expect_success 'cc trailer with various stripping opts' '
	cd send-email-repo &&
	patches=$(git format-patch -n HEAD^1) &&
	git send-email \
		--from="Example <nobody@example.com>" \
		--to=nobody@example.com \
		--cc="one@example.com" \
		--smtp-server="$(pwd)/fake.sendmail" \
		--confirm=never \
		$patches 2>errors &&
	test -f msgtxt1
'

test_expect_success 'setup expect for multiline patch' '
	cd send-email-repo &&
	rm -f commandline* msgtxt* &&
	echo "more content" >>file &&
	git add file &&
	git commit -m "Third commit with more content"
'

test_expect_success 'multiline subject' '
	cd send-email-repo &&
	patches=$(git format-patch -n HEAD~2..HEAD) &&
	test -n "$patches"
'

test_expect_success 'send-email --compose' '
	cd send-email-repo &&
	rm -f commandline* msgtxt* &&
	patches=$(git format-patch -n HEAD^1) &&
	git send-email \
		--compose \
		--subject="Test compose" \
		--from="Example <nobody@example.com>" \
		--to=nobody@example.com \
		--smtp-server="$(pwd)/fake.sendmail" \
		--confirm=never \
		$patches 2>errors &&
	test -f msgtxt1
'

test_expect_success 'send-email --validate hook' '
	cd send-email-repo &&
	rm -f commandline* msgtxt* &&
	mkdir -p .git/hooks &&
	write_script .git/hooks/sendemail-validate <<-\EOF &&
	exit 0
	EOF
	patches=$(git format-patch -n HEAD^1) &&
	git send-email \
		--validate \
		--from="Example <nobody@example.com>" \
		--to=nobody@example.com \
		--smtp-server="$(pwd)/fake.sendmail" \
		--confirm=never \
		$patches 2>errors &&
	test -f msgtxt1
'

test_expect_success 'send-email --to-cmd' '
	cd send-email-repo &&
	rm -f commandline* msgtxt* &&
	write_script to-cmd.sh <<-\EOF &&
	echo "tocmd@example.com"
	EOF
	patches=$(git format-patch -n HEAD^1) &&
	git send-email \
		--from="Example <nobody@example.com>" \
		--to-cmd="$(pwd)/to-cmd.sh" \
		--smtp-server="$(pwd)/fake.sendmail" \
		--confirm=never \
		$patches 2>errors &&
	test -f msgtxt1 &&
	grep "!tocmd@example.com!" commandline1
'

test_expect_success 'send-email --cc-cmd' '
	cd send-email-repo &&
	rm -f commandline* msgtxt* &&
	write_script cc-cmd.sh <<-\EOF &&
	echo "cccmd@example.com"
	EOF
	patches=$(git format-patch -n HEAD^1) &&
	git send-email \
		--from="Example <nobody@example.com>" \
		--to=nobody@example.com \
		--cc-cmd="$(pwd)/cc-cmd.sh" \
		--smtp-server="$(pwd)/fake.sendmail" \
		--confirm=never \
		$patches 2>errors &&
	test -f msgtxt1 &&
	grep "!cccmd@example.com!" commandline1
'

test_expect_success 'send-email --8bit-encoding' '
	cd send-email-repo &&
	rm -f commandline* msgtxt* &&
	patches=$(git format-patch -n HEAD^1) &&
	git send-email \
		--8bit-encoding=UTF-8 \
		--from="Example <nobody@example.com>" \
		--to=nobody@example.com \
		--smtp-server="$(pwd)/fake.sendmail" \
		--confirm=never \
		$patches 2>errors &&
	test -f msgtxt1 &&
	grep "Content-Type.*charset=UTF-8" msgtxt1
'

test_done
