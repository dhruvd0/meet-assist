fn main() {
    seal_bundled_keys();
    generate_tray_translations();

    // Linux ships transcribe-cpp as a shared libtranscribe + loadable ggml
    // backend modules (the `dynamic-backends` posture in Cargo.toml). Bake an
    // $ORIGIN-relative rpath into the `case` binary so it finds libtranscribe
    // next to it in the package — deb/rpm install into the app-private
    // `/usr/lib/CASE` (the dir tauri already uses for resources; keeps
    // CASE's libs out of the ldconfig-scanned `/usr/lib`, issue #1639) while
    // the AppImage keeps them in `usr/lib` (linuxdeploy's layout), hence both
    // entries. transcribe's
    // init_backends_default() then loads the ggml modules co-located there.
    // (Windows resolves DLLs from the exe directory, so it needs no rpath;
    // macOS links transcribe-cpp statically via the `metal` feature.)
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/CASE:$ORIGIN/../lib");
    }

    // CPAL (handy-recorder's and rodio's) links two CoreAudio functions that
    // only exist on macOS 14.2+. Weak-link the framework so CASE still
    // launches on older macOS (minimumSystemVersion is 10.15).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
    }

    // Stage transcribe-cpp's shared runtime libraries (and the dlopen'd ggml
    // backend modules) for the installer. Self-gates on the shared /
    // dynamic-backends posture used by Linux and Windows; it's a no-op for the
    // static macOS `metal` build, where there is nothing to ship.
    stage_transcribe_runtime_libs();

    // When ORT is dynamically linked (Windows CI sets ORT_LIB_LOCATION +
    // ORT_PREFER_DYNAMIC_LINK to a baseline ONNX Runtime), ship its onnxruntime.dll
    // next to CASE.exe so the app loads our baseline build instead of statically
    // embedding pyke's /arch:AVX2 one (which crashes at startup on pre-Haswell CPUs).
    stage_onnxruntime_dll();

    // Must run after transcribe staging because that helper recreates transcribe-libs/.
    stage_vc_runtime_dlls();

    tauri_build::build()
}

/// Stage the MSVC runtime DLLs into `transcribe-libs/` for app-local deployment.
///
/// CASE's native stack links the VC++ runtime dynamically (/MD). Shipping the
/// DLLs beside `case.exe` covers machines with no redistributable installed and
/// machines whose system redist is older than the CI toolset (issue #1527).
///
/// Driven by `CASE_VC_REDIST_DIRS`, set by CI to the redist dirs from the same
/// Visual Studio install that compiled the native code. Copies only the runtime
/// DLL families CASE imports and no-ops when the env var is unset.
fn stage_vc_runtime_dlls() {
    use std::path::PathBuf;

    println!("cargo:rerun-if-env-changed=CASE_VC_REDIST_DIRS");

    let Some(redist_dirs) = std::env::var_os("CASE_VC_REDIST_DIRS") else {
        return;
    };
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let dest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("transcribe-libs");
    std::fs::create_dir_all(&dest).expect("create transcribe-libs staging dir");

    let mut copied: Vec<String> = Vec::new();
    for dir in std::env::split_paths(&redist_dirs) {
        for entry in std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("CASE_VC_REDIST_DIRS: read {}: {e}", dir.display()))
            .flatten()
        {
            let src = entry.path();
            let name = src
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let lower = name.to_lowercase();
            let wanted = lower.ends_with(".dll")
                && (lower.starts_with("msvcp140")
                    || lower.starts_with("vcruntime140")
                    || lower.starts_with("vcomp140"));
            if wanted {
                std::fs::copy(&src, dest.join(&name))
                    .unwrap_or_else(|e| panic!("copy {}: {e}", src.display()));
                copied.push(lower);
            }
        }
    }

    // Fail the build rather than ship an installer that regresses issue #1527.
    for required in ["msvcp140.dll", "vcruntime140.dll"] {
        if !copied.iter().any(|n| n == required) {
            panic!(
                "CASE_VC_REDIST_DIRS is set but {required} was not found in it; \
                 the app-local VC++ runtime would be incomplete and CASE would \
                 crash on machines without a current redist (issue #1527)"
            );
        }
    }
    println!(
        "cargo:warning=Staged {} VC++ runtime DLL(s) for app-local deployment",
        copied.len()
    );
}

/// Copy the dynamically-linked ONNX Runtime `onnxruntime.dll` into the
/// `transcribe-libs/` staging dir so `tauri.windows.conf.json` bundles it beside
/// `CASE.exe` (Windows resolves DLLs from the executable's directory).
///
/// No-op unless `ORT_PREFER_DYNAMIC_LINK` + `ORT_LIB_LOCATION` are set for a Windows
/// target — i.e. the CI dynamic-link path. A plain static build (no env) skips this
/// and keeps the embedded ORT, and non-Windows targets bundle their ORT elsewhere
/// (see build.yml frameworks/deb.files steps), so they are ignored here.
fn stage_onnxruntime_dll() {
    use std::path::PathBuf;

    println!("cargo:rerun-if-env-changed=ORT_LIB_LOCATION");
    println!("cargo:rerun-if-env-changed=ORT_PREFER_DYNAMIC_LINK");

    if std::env::var_os("ORT_PREFER_DYNAMIC_LINK").is_none() {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Some(lib_location) = std::env::var_os("ORT_LIB_LOCATION") else {
        return;
    };

    let src = PathBuf::from(&lib_location).join("onnxruntime.dll");
    if !src.exists() {
        panic!(
            "ORT_PREFER_DYNAMIC_LINK is set but {} does not exist; a dynamic ORT \
             build must supply onnxruntime.dll to bundle",
            src.display()
        );
    }

    // transcribe-libs/ is already created by stage_transcribe_runtime_libs() on the
    // Windows x86_64 dynamic-backends build and bundled by tauri.windows.conf.json;
    // create it defensively so this is self-contained.
    let dest_dir =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("transcribe-libs");
    std::fs::create_dir_all(&dest_dir).expect("create transcribe-libs staging dir");
    std::fs::copy(&src, dest_dir.join("onnxruntime.dll"))
        .unwrap_or_else(|e| panic!("copy {}: {e}", src.display()));
    println!("cargo:warning=Staged onnxruntime.dll for Windows bundling");
}

/// Stage transcribe-cpp's shared runtime libraries into `transcribe-libs/` so the
/// installer can ship them next to the executable. One code path covers Windows
/// (`.dll`) and Linux (versioned `.so`); the match-by-name filter below handles
/// both naming schemes.
///
/// Source dirs arrive as `DEP_TRANSCRIBE_CPP_*`: the sys crate (`links =
/// "transcribe"`) emits its install dirs and the wrapper (`links =
/// "transcribe_cpp"`) forwards them one hop to us — the only way that metadata
/// crosses cargo's one-hop `links` boundary. The keys exist only in a shared /
/// dynamic-backends build; a static build (macOS `metal`) leaves them unset, so
/// this is a no-op there. `RUNTIME_DIR` (core libs) and `MODULE_DIR` (dlopen'd
/// ggml modules) may be the same dir — the `BTreeSet` below dedups them.
///
/// Where the staged dir lands: Windows bundles it beside `case.exe` (DLLs resolve
/// from the exe dir); Linux deb/rpm map it into the app-private `/usr/lib/CASE`
/// and the AppImage into `usr/lib`, both on the binary's rpath.
fn stage_transcribe_runtime_libs() {
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    println!("cargo:rerun-if-env-changed=DEP_TRANSCRIBE_CPP_RUNTIME_DIR");
    println!("cargo:rerun-if-env-changed=DEP_TRANSCRIBE_CPP_MODULE_DIR");

    // Present only in a shared posture. A static build has nothing to ship.
    let Some(runtime_dir) = std::env::var_os("DEP_TRANSCRIBE_CPP_RUNTIME_DIR") else {
        return;
    };

    // transcribe-cpp publishes its runtime layout in up to two directories:
    //   RUNTIME_DIR : the shared libs to load (transcribe + core ggml / ggml-base)
    //   MODULE_DIR  : the dlopen'd ggml backend modules (the per-ISA ggml-cpu-*
    //                 and ggml-vulkan), dynamic-backends only. Often — but not
    //                 always — the SAME directory as RUNTIME_DIR (it is on Linux).
    // BOTH must sit next to the executable, or init_backends_default() finds the
    // core libs but zero loadable compute backends and registers no devices.
    let mut dirs = BTreeSet::new();
    dirs.insert(PathBuf::from(runtime_dir));
    if let Some(module_dir) = std::env::var_os("DEP_TRANSCRIBE_CPP_MODULE_DIR") {
        dirs.insert(PathBuf::from(module_dir));
    }

    let dest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("transcribe-libs");
    // Recreate clean so a renamed or dropped ggml module can never linger in the
    // package from a previous build.
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).expect("create transcribe-libs staging dir");

    // Collect every candidate library name first (across both dirs) so the
    // pruning below can see each lib's whole symlink family at once.
    let mut libs: std::collections::BTreeMap<String, PathBuf> = Default::default();
    for dir in &dirs {
        println!("cargo:rerun-if-changed={}", dir.display());
        for entry in std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .flatten()
        {
            let src = entry.path();
            let name = src.file_name().and_then(|s| s.to_str()).unwrap_or("");
            // Match by NAME, not extension: Linux versions its libs
            // (libtranscribe.so.0, .so.0.2.0) and the loader needs the SONAME, so
            // an extension-only filter would miss the versioned names entirely.
            let is_lib = name.ends_with(".dll")
                || name.ends_with(".dylib")
                || name.ends_with(".so")
                || name.contains(".so.");
            if is_lib {
                libs.insert(name.to_string(), src);
            }
        }
    }

    // A Linux install dir carries each lib as a symlink chain (for example,
    // libfoo.so -> libfoo.so.0.2 -> libfoo.so.0.2.0), and tauri's deb/rpm
    // bundlers flatten symlinks into real files. Staging every name would
    // triplicate each lib and draw "not a symbolic link" warnings from ldconfig
    // (issue #1639). Only one name per lib is needed at runtime: the shortest
    // versioned name is the SONAME for linked core libs, while a dlopen'd ggml
    // backend module generally has only its bare unversioned name. Stage that
    // name; `fs::copy` dereferences the symlink so the staged file is real.
    let mut best: std::collections::BTreeMap<&str, (&str, &PathBuf, usize)> = Default::default();
    for (name, src) in &libs {
        let (stem, rank) = match split_versioned_so(name) {
            // Windows/macOS names (.dll/.dylib) are unversioned: keep as-is.
            None => (name.as_str(), 0),
            // Prefer the shortest versioned name (`.so.0`, `.so.0.2`, etc.),
            // then the bare `.so`; a full version is only the fallback when the
            // install tree did not provide its SONAME symlink.
            Some((stem, 0)) => (stem, usize::MAX),
            Some((stem, depth)) => (stem, depth - 1),
        };
        match best.get(stem) {
            Some(&(_, _, existing)) if existing <= rank => {}
            _ => {
                best.insert(stem, (name, src, rank));
            }
        }
    }

    let mut copied = 0usize;
    for &(name, src, _) in best.values() {
        std::fs::copy(src, dest.join(name))
            .unwrap_or_else(|e| panic!("copy {}: {e}", src.display()));
        copied += 1;
    }
    if copied == 0 {
        panic!(
            "no transcribe-cpp runtime libraries found under {dirs:?}; a shared / \
             dynamic-backends build must ship them or the app registers zero \
             compute devices"
        );
    }
    println!("cargo:warning=Staged {copied} transcribe-cpp runtime library file(s)");
}

/// Split a versioned ELF shared-library name into (stem, version depth):
/// `libfoo.so` -> ("libfoo", 0), `libfoo.so.0` -> ("libfoo", 1),
/// `libfoo.so.0.2.0` -> ("libfoo", 3). Returns None for names that aren't a
/// `.so` optionally followed by dot-separated numeric components.
fn split_versioned_so(name: &str) -> Option<(&str, usize)> {
    let idx = name.find(".so")?;
    let (stem, rest) = (&name[..idx], &name[idx + 3..]);
    if rest.is_empty() {
        return Some((stem, 0));
    }
    let comps: Vec<&str> = rest.strip_prefix('.')?.split('.').collect();
    comps
        .iter()
        .all(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_digit()))
        .then_some((stem, comps.len()))
}

/// Generate tray menu translations from frontend locale files.
///
/// Source of truth: src/i18n/locales/*/translation.json
/// The English "tray" section defines the struct fields.
fn generate_tray_translations() {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let locales_dir = Path::new("../src/i18n/locales");

    println!("cargo:rerun-if-changed=../src/i18n/locales");

    // Collect all locale translations
    let mut translations: BTreeMap<String, serde_json::Value> = BTreeMap::new();

    for entry in fs::read_dir(locales_dir).unwrap().flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let lang = path.file_name().unwrap().to_str().unwrap().to_string();
        let json_path = path.join("translation.json");

        println!("cargo:rerun-if-changed={}", json_path.display());

        let content = fs::read_to_string(&json_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        if let Some(tray) = parsed.get("tray").cloned() {
            translations.insert(lang, tray);
        }
    }

    // English defines the schema
    let english = translations.get("en").unwrap().as_object().unwrap();
    let fields: Vec<_> = english
        .keys()
        .map(|k| (camel_to_snake(k), k.clone()))
        .collect();

    // Generate code
    let mut out = String::from(
        "// Auto-generated from src/i18n/locales/*/translation.json - do not edit\n\n",
    );

    // Struct
    out.push_str("#[derive(Debug, Clone)]\npub struct TrayStrings {\n");
    for (rust_field, _) in &fields {
        out.push_str(&format!("    pub {rust_field}: String,\n"));
    }
    out.push_str("}\n\n");

    // Static map
    out.push_str(
        "pub static TRANSLATIONS: Lazy<HashMap<&'static str, TrayStrings>> = Lazy::new(|| {\n",
    );
    out.push_str("    let mut m = HashMap::new();\n");

    for (lang, tray) in &translations {
        out.push_str(&format!("    m.insert(\"{lang}\", TrayStrings {{\n"));
        for (rust_field, json_key) in &fields {
            let val = tray
                .get(json_key)
                .and_then(|v| v.as_str())
                .or_else(|| english.get(json_key).and_then(|v| v.as_str()))
                .unwrap_or("");
            out.push_str(&format!(
                "        {rust_field}: \"{}\".to_string(),\n",
                escape_string(val)
            ));
        }
        out.push_str("    });\n");
    }

    out.push_str("    m\n});\n");

    fs::write(Path::new(&out_dir).join("tray_translations.rs"), out).unwrap();

    println!(
        "cargo:warning=Generated tray translations: {} languages, {} fields",
        translations.len(),
        fields.len()
    );
}

fn camel_to_snake(s: &str) -> String {
    s.chars()
        .enumerate()
        .fold(String::new(), |mut acc, (i, c)| {
            if c.is_uppercase() && i > 0 {
                acc.push('_');
            }
            acc.push(c.to_lowercase().next().unwrap());
            acc
        })
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn seal_bundled_keys() {
    use ring::{
        aead,
        rand::{SecureRandom, SystemRandom},
    };
    use std::{collections::HashMap, env, path::PathBuf};

    let dotenv = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("../.env");
    if dotenv.exists() {
        println!("cargo:rerun-if-changed={}", dotenv.display());
    }
    for name in [
        "CASE_OPENROUTER_KEY",
        "CASE_GROQ_KEY",
        "GROQ_API_KEY",
        "CASE_JEV_KEY",
        "JEV_API_KEY",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let file_vars: HashMap<String, String> = if dotenv.exists() {
        dotenvy::from_path_iter(&dotenv)
            .unwrap_or_else(|_| panic!("Cannot read root .env"))
            .map(|entry| entry.unwrap_or_else(|_| panic!("Invalid root .env syntax")))
            .filter(|(name, _)| {
                matches!(
                    name.as_str(),
                    "CASE_OPENROUTER_KEY"
                        | "CASE_GROQ_KEY"
                        | "GROQ_API_KEY"
                        | "CASE_JEV_KEY"
                        | "JEV_API_KEY"
                )
            })
            .collect()
    } else {
        HashMap::new()
    };
    let env_or_file = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| env::var(name).ok())
            .or_else(|| names.iter().find_map(|name| file_vars.get(*name).cloned()))
            .filter(|value| !value.trim().is_empty())
    };
    let rng = SystemRandom::new();
    let mut key_bytes = [0u8; 32];
    rng.fill(&mut key_bytes)
        .expect("credential sealing randomness");
    let key = aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, &key_bytes).expect("credential sealing key"),
    );
    let mut generated = format!("const SEALING_KEY: [u8; 32] = {key_bytes:?};\n");
    for (service, names) in [
        ("OPENROUTER", &["CASE_OPENROUTER_KEY"][..]),
        ("GROQ", &["CASE_GROQ_KEY", "GROQ_API_KEY"][..]),
        ("JEV", &["CASE_JEV_KEY", "JEV_API_KEY"][..]),
    ] {
        if let Some(value) = env_or_file(names) {
            let mut nonce = [0u8; 12];
            rng.fill(&mut nonce).expect("credential sealing nonce");
            let mut ciphertext = value.into_bytes();
            let aad = format!("case-bundled-v1:{service}");
            key.seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(aad.as_bytes()),
                &mut ciphertext,
            )
            .expect("seal bundled credential");
            generated.push_str(&format!("const {service}: Option<Sealed> = Some(Sealed {{ nonce: {nonce:?}, ciphertext: &{ciphertext:?} }});\n"));
        } else {
            generated.push_str(&format!("const {service}: Option<Sealed> = None;\n"));
        }
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory"));
    std::fs::write(out.join("bundled_keys.rs"), generated).expect("write sealed defaults");
}
