use std::{env, fs, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-env-changed=OFF42_OCR_SDK");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    // GNU source checks need no SDK. Linking/package creation requires a matching static SDK.
    if !msvc && env::var_os("OFF42_OCR_SDK").is_none() {
        return;
    }
    // Windows builds must provision the pinned static SDK; never ship an unavailable stub.
    let sdk = PathBuf::from(
        env::var_os("OFF42_OCR_SDK").expect("Run scripts/build-ocr.ps1 and set OFF42_OCR_SDK"),
    );
    let libs = sdk.join("lib");
    println!("cargo:rustc-link-search=native={}", libs.display());
    let mut names: Vec<_> = fs::read_dir(&libs)
        .expect("OCR SDK lib directory")
        .map(|entry| entry.unwrap().path())
        .filter(|p| {
            p.extension()
                .is_some_and(|s| s == if msvc { "lib" } else { "a" })
        })
        .collect();
    names.sort();
    assert!(
        names
            .iter()
            .any(|p| p.file_stem().unwrap() == if msvc { "off42_ocr" } else { "liboff42_ocr" }),
        "Missing OCR initialization shim"
    );
    for path in names {
        println!("cargo:rerun-if-changed={}", path.display());
        println!(
            "cargo:rustc-link-lib=static={}",
            if msvc {
                path.file_stem().unwrap().to_str().unwrap()
            } else {
                path.file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .trim_start_matches("lib")
            }
        );
    }
    if !msvc {
        println!("cargo:rustc-link-lib=static=stdc++");
    }
    for lib in [
        "ws2_32", "user32", "advapi32", "shell32", "ole32", "uuid", "bcrypt",
    ] {
        println!("cargo:rustc-link-lib={lib}");
    }
}
