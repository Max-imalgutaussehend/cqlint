//! `cqlint`: a very fast linter and formatter for CQL.
//!
//! * [`format::format`] rewrites CQL into a canonical layout and refuses to change the token stream.
//! * [`rules::lint`] reports performance and safety problems with stable `CQLINT` codes.
pub mod config;
pub mod doc;
pub mod format;
pub mod rules;
pub mod schema;
