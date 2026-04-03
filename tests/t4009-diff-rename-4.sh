#!/bin/sh
#
# Copyright (c) 2005 Junio C Hamano
#

test_description='Same rename detection as t4003 but testing diff-raw -z.'

. ./test-lib.sh

COPYING_test_data () {
	cat <<\EOF

 Note that the only valid version of the GPL as far as this project
 is concerned is _this_ particular version of the license (ie v2, not
 v2.2 or v3.x or whatever), unless explicitly otherwise stated.

 HOWEVER, in order to allow a migration to GPLv3 if that seems like
 a good idea, I also ask that people involved with the project make
 their preferences known. In particular, if you trust me to make that
 decision, you might note so in your copyright message, ie something
 like

	This file is licensed under the GPL v2, or a later version
	at the discretion of Linus.

  might avoid issues. But we can also just decide to synchronize and
  contact all copyright holders on record if/when the occasion arises.

			Linus Torvalds
EOF
}

test_expect_success 'prepare reference tree' '
	git init &&
	git config user.email test@test.com &&
	git config user.name "Test User" &&
	COPYING_test_data >COPYING &&
	echo frotz >rezrov &&
	git update-index --add COPYING rezrov &&
	tree=$(git write-tree) &&
	echo $tree
'

test_expect_success 'prepare work tree' '
	sed -e "s/HOWEVER/However/" <COPYING >COPYING.1 &&
	sed -e "s/GPL/G.P.L/g" <COPYING >COPYING.2 &&
	rm -f COPYING &&
	git update-index --add --remove COPYING COPYING.1 COPYING.2
'

test_expect_failure 'validate output from rename/copy detection (#1) (not implemented)' '
	git diff-index -z -C $tree >current &&
	test -s current
'

test_expect_success 'prepare work tree again' '
	mv COPYING.2 COPYING &&
	git update-index --add --remove COPYING COPYING.1 COPYING.2
'

test_expect_failure 'validate output from rename/copy detection (#2) (not implemented)' '
	git diff-index -z -C $tree >current &&
	test -s current
'

test_expect_success 'prepare work tree once again' '
	COPYING_test_data >COPYING &&
	git update-index --add --remove COPYING COPYING.1
'

test_expect_failure 'validate output from rename/copy detection (#3) (not implemented)' '
	git diff-index -z -C --find-copies-harder $tree >current &&
	test -s current
'

test_done
