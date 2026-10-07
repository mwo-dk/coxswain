COMMENT =		two-panel file manager for the terminal

V =			2.8.1
DISTNAME =		coxswain-${V}

# The crates.io release: the terminal app alone, with its own Cargo.lock.
DISTFILES =		${DISTNAME}${EXTRACT_SUFX}{coxswain/${V}/download}
SITES =			https://crates.io/api/v1/crates/

CATEGORIES =		sysutils

HOMEPAGE =		https://github.com/mwo-dk/coxswain

MAINTAINER =		Michael W. Olesen <michael.w.olesen@gmail.com>

# MIT
PERMIT_PACKAGE =	Yes

WANTLIB +=		${MODCARGO_WANTLIB} m onig sqlite3

MODULES =		devel/cargo

LIB_DEPENDS =		databases/sqlite3 \
			textproc/oniguruma

CONFIGURE_STYLE =	cargo

SEPARATE_BUILD =	Yes

# cargo install puts two copies of the program in bin; cox becomes a link.
post-install:
	ln -sf coxswain ${PREFIX}/bin/cox
	${INSTALL_MAN} ${WRKSRC}/coxswain.1 ${PREFIX}/man/man1/

.include "crates.inc"

.include <bsd.port.mk>
