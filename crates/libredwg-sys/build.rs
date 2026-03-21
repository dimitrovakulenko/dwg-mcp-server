use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=LIBREDWG_ROOT_DIR");
    println!("cargo:rerun-if-env-changed=LIBREDWG_INCLUDE_DIR");
    println!("cargo:rerun-if-env-changed=LIBREDWG_LIB_DIR");
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-changed=bridge.h");
    println!("cargo:rerun-if-changed=bridge.c");

    let linkage = discover_linkage();

    for include_dir in &linkage.include_dirs {
        println!("cargo:include={}", include_dir.display());
    }
    println!("cargo:rustc-link-search=native={}", linkage.lib_dir.display());
    println!(
        "cargo:rustc-link-search=native={}",
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must exist")).display()
    );
    println!("cargo:rustc-link-lib=static=dwg_bridge");
    if linkage.static_link {
        println!("cargo:rustc-link-lib=static=redwg");
    } else {
        println!("cargo:rustc-link-lib=redwg");
    }
    println!("cargo:rustc-link-lib=m");

    compile_bridge(&linkage);
    generate_bindings(&linkage);
}

struct Linkage {
    include_dirs: Vec<PathBuf>,
    lib_dir: PathBuf,
    static_link: bool,
}

fn discover_linkage() -> Linkage {
    if let Some(root_dir) = env::var_os("LIBREDWG_ROOT_DIR").map(PathBuf::from) {
        return Linkage {
            include_dirs: vec![root_dir.join("src"), root_dir.join("include")],
            lib_dir: root_dir.join("src/.libs"),
            static_link: true,
        };
    }

    match (
        env::var_os("LIBREDWG_INCLUDE_DIR").map(PathBuf::from),
        env::var_os("LIBREDWG_LIB_DIR").map(PathBuf::from),
    ) {
        (Some(include_dir), Some(lib_dir)) => Linkage {
            include_dirs: vec![include_dir],
            lib_dir,
            static_link: false,
        },
        _ => discover_pkg_config().unwrap_or_else(discover_vendored),
    }
}

fn discover_pkg_config() -> Option<Linkage> {
    let library = pkg_config::Config::new().probe("libredwg").ok()?;
    let lib_dir = library.link_paths.into_iter().next()?;

    Some(Linkage {
        include_dirs: library.include_paths,
        lib_dir,
        static_link: false,
    })
}

fn discover_vendored() -> Linkage {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("third_party/libredwg");
    let config_dir = root.join("src");
    let include_dir = root.join("include");
    let lib_dir = root.join("src/.libs");

    Linkage {
        include_dirs: vec![config_dir, include_dir],
        lib_dir,
        static_link: true,
    }
}

fn compile_bridge(linkage: &Linkage) {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must exist"));
    let object_path = out_dir.join("bridge.o");
    let archive_path = out_dir.join("libdwg_bridge.a");

    let mut compile = Command::new("cc");
    compile
        .arg("-c")
        .arg(Path::new("bridge.c"))
        .arg("-o")
        .arg(&object_path)
        .arg("-std=c11");

    for include_dir in &linkage.include_dirs {
        compile.arg(format!("-I{}", include_dir.display()));
    }

    let status = compile.status().expect("failed to execute cc for bridge.c");
    if !status.success() {
        panic!("failed to compile bridge.c");
    }

    let status = Command::new("ar")
        .arg("crs")
        .arg(&archive_path)
        .arg(&object_path)
        .status()
        .expect("failed to execute ar for bridge archive");
    if !status.success() {
        panic!("failed to archive bridge.o");
    }
}

fn generate_bindings(linkage: &Linkage) {
    let wrapper = Path::new("wrapper.h");
    let mut builder = bindgen::Builder::default()
        .header(
            wrapper
                .to_str()
                .expect("wrapper.h path should always be valid UTF-8"),
        )
        .allowlist_function("dwg_.*")
        .allowlist_function("bridge_.*")
        .allowlist_type("Dwg_.*")
        .allowlist_type("dwg_.*")
        .allowlist_type("Bridge.*")
        .allowlist_var("DWG_.*")
        .generate_cstr(true);

    for include_dir in &linkage.include_dirs {
        builder = builder.clang_arg(format!("-I{}", include_dir.display()));
    }

    let bindings = builder
        .generate()
        .expect("failed to generate libredwg bindings");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR must exist"));
    bindings
        .write_to_file(out_dir.join("bindings.rs"))
        .expect("failed to write libredwg bindings");
}
