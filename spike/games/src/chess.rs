//! Coxswain's own PGN lexer and parser, compiled as they are (by path), and a copy of the SCID
//! reader changed only to read from bytes instead of files (a plugin gets bytes, never a path).
#[path = "../../../crates/coxswain-core/src/chess/lex.rs"]
pub mod lex;
#[path = "../../../crates/coxswain-core/src/chess/pgn.rs"]
pub mod pgn;
pub mod scid;

/// What pgn.rs and scid.rs take from games.rs and review.rs.
pub mod games {
    pub(crate) fn text(b: &[u8]) -> std::borrow::Cow<'_, str> {
        match std::str::from_utf8(b) {
            Ok(s) => s.into(),
            Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(b).0,
        }
    }
}
pub mod review {
    pub const CAP: i32 = 1000;
}
