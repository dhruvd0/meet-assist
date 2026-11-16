//! Optional cloud sync: accounts, the background sync loop, and the
//! SQLite change log it drains. Local data stays the source of truth.

pub mod audio;
pub mod auth;
pub mod local;
pub mod service;
pub mod sync;

use std::sync::OnceLock;

const SUPABASE_URL: &str = "https://fjohdmmgmvcjdoswkvsq.supabase.co";
/// Publishable by design: row-level security, not this key, guards the data.
const ANON_KEY: &str = "sb_publishable_81VLvYMSAX1a8yPqhQzsaA_bqP4VJob";

/// Debug builds can point at another Supabase (a local stack) with
/// `CASE_SUPABASE_URL` and `CASE_SUPABASE_ANON_KEY`.
fn setting(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|_| cfg!(debug_assertions))
        .unwrap_or_else(|| default.to_string())
}

pub fn supabase_url() -> &'static str {
    static URL: OnceLock<String> = OnceLock::new();
    URL.get_or_init(|| setting("CASE_SUPABASE_URL", SUPABASE_URL))
}

pub fn anon_key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| setting("CASE_SUPABASE_ANON_KEY", ANON_KEY))
}
