#
# This file and its contents are supplied under the terms of the
# Common Development and Distribution License ("CDDL"). You may
# only use this file in accordance with the terms of the CDDL.
#
# A full copy of the text of the CDDL should have accompanied this
# source. A copy of the CDDL is also available via the Internet at
# http://www.illumos.org/license/CDDL.
#

#
# Copyright 2026 Michael W. Olesen
#

BUILD_BITS=64
BUILD_STYLE=cargo
include ../../../make-rules/shared-macros.mk

# The crates.io release: the terminal app alone, with its own Cargo.lock
# (the GitHub tarball is a workspace with the desktop app in it).
COMPONENT_NAME=		coxswain
HUMAN_VERSION=		2.8.0
COMPONENT_SUMMARY=	Two-panel file manager for the terminal, Norton Commander style
COMPONENT_PROJECT_URL=	https://github.com/mwo-dk/coxswain
COMPONENT_FMRI=		file/coxswain
COMPONENT_CLASSIFICATION=	Applications/System Utilities
COMPONENT_ARCHIVE_HASH=	sha256:0000000000000000000000000000000000000000000000000000000000000000
COMPONENT_LICENSE=	MIT
# The crate carries no LICENSE file; this is a copy of the repository's.
COMPONENT_LICENSE_FILE=	$(COMPONENT_NAME).license

TEST_TARGET=	$(NO_TESTS)
include $(WS_MAKE_RULES)/common.mk

# The manual page. cargo install puts two copies of the program in bin,
# coxswain and cox; the package makes cox a link instead.
COMPONENT_POST_INSTALL_ACTION += \
	$(RM) $(PROTOUSRBINDIR)/cox ; \
	$(MKDIR) $(PROTOUSRSHAREMAN1DIR) ; \
	$(CP) $(SOURCE_DIR)/coxswain.1 $(PROTOUSRSHAREMAN1DIR)/coxswain.1

# Auto-generated dependencies: run `gmake REQUIRED_PACKAGES` after the first build.
REQUIRED_PACKAGES += $(GCC_RUNTIME_PKG)
REQUIRED_PACKAGES += system/library
REQUIRED_PACKAGES += system/library/math
