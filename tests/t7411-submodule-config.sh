#!/bin/sh
#
# Ported from git/t/t7411-submodule-config.sh
# Tests submodule config cache infrastructure
# Note: upstream uses test-tool extensively. We test what we can via
# submodule init/status/update commands.
#

test_description='Test submodules config cache infrastructure'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'submodule config cache setup' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	mkdir submodule &&
	(cd submodule &&
		"$REAL_GIT" init &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo a >a &&
		"$REAL_GIT" add . &&
		"$REAL_GIT" commit -ma
	) &&
	mkdir super &&
	(cd super &&
		"$REAL_GIT" init &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		"$REAL_GIT" submodule add ../submodule &&
		"$REAL_GIT" submodule add ../submodule a &&
		"$REAL_GIT" commit -m "add as submodule and a"
	)
'

test_expect_success 'submodule status shows configured submodules' '
	cd super &&
	git submodule status >actual &&
	grep submodule actual &&
	grep "^." actual
'

test_expect_success 'submodule init configures URLs' '
	"$REAL_GIT" clone super super-clone &&
	(
		cd super-clone &&
		git submodule init &&
		git config submodule.submodule.url >actual &&
		test -s actual &&
		git config submodule.a.url >actual &&
		test -s actual
	)
'

test_expect_success 'submodule update checks out correct commits' '
	cd super-clone &&
	git submodule update --init &&
	test -f submodule/a &&
	test -f a/a
'

test_done
