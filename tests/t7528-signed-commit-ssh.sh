#!/bin/sh
# Ported from upstream git t7528-signed-commit-ssh.sh
# SSH signing not available; structure tests plus GPG commit signing tests.

test_description='signed commit structure and GPG signing tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Initialize a repo in the trash directory
git init --quiet

# Set up GPG if available
if command -v gpg >/dev/null 2>&1 && test -d /home/hasi/.gnupg-test
then
	GNUPGHOME=/home/hasi/.gnupg-test
	export GNUPGHOME
	GPG_KEY=F949CA6F7DED01649C901F84B5296A310BC8A4DE
	test_set_prereq GPG
fi

SYSTEM_GIT=/usr/bin/git

test_expect_success 'setup' '
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo content >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'unsigned commit has no gpgsig header' '
	git cat-file -p HEAD >actual &&
	! grep "^gpgsig" actual
'

test_expect_success 'log shows commit without signature info' '
	git log --oneline >actual &&
	test_line_count = 1 actual
'

test_expect_success 'multiple commits' '
	echo more >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m second &&
	git log --oneline >actual &&
	test_line_count = 2 actual
'

test_expect_success 'cat-file shows tree and parent' '
	git cat-file -p HEAD >actual &&
	grep "^tree " actual &&
	grep "^parent " actual &&
	grep "^author " actual
'

test_expect_success 'rev-list works' '
	git rev-list HEAD >actual &&
	test_line_count = 2 actual
'

# ── GPG commit signing tests ──────────────────────────────────────────────

test_expect_success GPG 'create GPG signed commit' '
	git config user.signingkey $GPG_KEY &&
	echo signed >signed-file &&
	git add signed-file &&
	test_tick &&
	$SYSTEM_GIT commit -S -m "gpg signed commit"
'

test_expect_success GPG 'signed commit has gpgsig header' '
	git cat-file -p HEAD >actual &&
	grep "^gpgsig " actual
'

test_expect_success GPG 'signed commit contains PGP signature' '
	git cat-file -p HEAD >actual &&
	grep "BEGIN PGP SIGNATURE" actual
'

test_expect_success GPG 'verify-commit succeeds on signed commit' '
	git verify-commit HEAD
'

test_expect_success GPG 'unsigned commit still has no gpgsig' '
	git cat-file -p HEAD^ >actual &&
	! grep "^gpgsig" actual
'

test_expect_success GPG 'amend with signature' '
	$SYSTEM_GIT commit --amend -S --no-edit &&
	git cat-file -p HEAD >actual &&
	grep "^gpgsig " actual
'

test_done
