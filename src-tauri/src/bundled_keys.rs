use ring::aead;
use std::sync::OnceLock;

struct Sealed {
    nonce: [u8; 12],
    ciphertext: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/bundled_keys.rs"));

fn open(sealed: &Option<Sealed>, service: &str) -> Option<String> {
    let sealed = sealed.as_ref()?;
    let key = aead::LessSafeKey::new(aead::UnboundKey::new(&aead::AES_256_GCM, &SEALING_KEY).ok()?);
    let mut bytes = sealed.ciphertext.to_vec();
    let aad = format!("case-bundled-v1:{service}");
    let plaintext = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(sealed.nonce),
            aead::Aad::from(aad.as_bytes()),
            &mut bytes,
        )
        .ok()?;
    String::from_utf8(plaintext.to_vec()).ok()
}

pub fn openrouter() -> Option<&'static str> {
    static KEY: OnceLock<Option<String>> = OnceLock::new();
    KEY.get_or_init(|| open(&OPENROUTER, "OPENROUTER"))
        .as_deref()
}

pub fn jev() -> Option<&'static str> {
    static KEY: OnceLock<Option<String>> = OnceLock::new();
    KEY.get_or_init(|| open(&JEV, "JEV")).as_deref()
}

pub fn groq() -> Option<&'static str> {
    static KEY: OnceLock<Option<String>> = OnceLock::new();
    KEY.get_or_init(|| open(&GROQ, "GROQ")).as_deref()
}

pub fn for_provider(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "groq" => groq(),
        "openrouter" => openrouter(),
        _ => None,
    }
}
