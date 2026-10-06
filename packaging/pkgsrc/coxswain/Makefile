# $NetBSD$

DISTNAME=	coxswain-2.3.0
CATEGORIES=	sysutils
EXTRACT_SUFX=	.crate

MAINTAINER=	michael.w.olesen@gmail.com
HOMEPAGE=	https://github.com/mwo-dk/coxswain
COMMENT=	Two-panel file manager for the terminal, Norton Commander style
LICENSE=	mit

.include "cargo-depends.mk"

# The search helper for one user, from boot: rc.d on the BSDs, SMF on illumos.
RCD_SCRIPTS=	coxswain_index
SMF_NAME=	coxswain-index

INSTALLATION_DIRS=	${PKGMANDIR}/man1

# cargo install puts two copies of the program in bin; cox becomes a link.
post-install:
	${RM} -f ${DESTDIR}${PREFIX}/bin/cox
	${LN} -s coxswain ${DESTDIR}${PREFIX}/bin/cox
	${INSTALL_MAN} ${WRKSRC}/coxswain.1 \
		${DESTDIR}${PREFIX}/${PKGMANDIR}/man1/coxswain.1

.include "../../lang/rust/cargo.mk"
.include "../../mk/bsd.pkg.mk"
