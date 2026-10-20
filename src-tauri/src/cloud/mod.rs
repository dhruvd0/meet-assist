//! Optional cloud sync: accounts, the background sync loop, and the
//! SQLite change log it drains. Local data stays the source of truth.

pub mod auth;
pub mod local;
pub mod service;
pub mod sync;

pub const SUPABASE_URL: &str = "https://fjohdmmgmvcjdoswkvsq.supabase.co";
/// Publishable by design: row-level security, not this key, guards the data.
pub const ANON_KEY: &str = "sb_publishable_81VLvYMSAX1a8yPqhQzsaA_bqP4VJob";
