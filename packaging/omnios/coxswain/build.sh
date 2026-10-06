#!/usr/bin/bash
#
# {{{ CDDL HEADER
#
# This file and its contents are supplied under the terms of the
# Common Development and Distribution License ("CDDL"), version 1.0.
# You may only use this file in accordance with the terms of version
# 1.0 of the CDDL.
#
# A full copy of the text of the CDDL should have accompanied this
# source. A copy of the CDDL is also available via the Internet at
# http://www.illumos.org/license/CDDL.
# }}}

# Copyright 2026 Michael W. Olesen

. ../../lib/build.sh

PROG=coxswain
VER=2.6.0
PKG=ooce/application/coxswain
SUMMARY="Two-panel file manager for the terminal"
DESC="A Norton Commander style file manager: two panels, git status per file, "
DESC+="archives as folders, ZFS snapshots as folders and search by name and text"

BUILD_DEPENDS_IPS=ooce/developer/rust

set_arch 64

XFORM_ARGS="-DPREFIX=${PREFIX#/}"

post_install() {
    typeset arch=$1

    destdir=$DESTDIR
    cross_arch $arch && destdir+=.$arch

    logcmd $LN -sf coxswain $destdir$PREFIX/bin/cox \
        || logerr "linking cox failed"
}

init
download_source $PROG v$VER
patch_source
prep_build
build_rust -p coxswain --locked
install_rust
xform files/coxswain-index-template.xml > $TMPDIR/coxswain-index.xml
install_smf application coxswain-index.xml
strip_install
make_package
clean_up

# Vim hints
# vim:ts=4:sw=4:et:fdm=marker
