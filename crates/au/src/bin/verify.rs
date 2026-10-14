//! muse-au-verify: loads a built `.component` the way a host does and exercises it.
//!
//! ```text
//! muse-au-verify PLUGIN.component FACTORY SUBTYPE MANUFACTURER
//! ```
//!
//! It prints one `CHECK name PASS|FAIL detail` line per check and a total, and exits with 1 if any check
//! failed. The factory name and the two four-character codes come from the component's `Info.plist`.

#[cfg(target_os = "macos")]
mod mac {
    use muse_au::check::{description, run, Report};
    use muse_au::sys::FactoryFunction;
    use std::ffi::{c_char, c_int, c_void, CStr, CString};
    use std::path::Path;

    extern "C" {
        fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> c_int;
        fn dlerror() -> *const c_char;
    }

    const RTLD_NOW: c_int = 2;

    fn library(path: &Path) -> Result<std::path::PathBuf, String> {
        if path.is_dir() {
            let dir = path.join("Contents/MacOS");
            let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
            for e in entries.flatten() {
                if e.path().is_file() {
                    return Ok(e.path());
                }
            }
            return Err(format!("no executable in {}", dir.display()));
        }
        Ok(path.to_path_buf())
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        if args.len() != 5 {
            eprintln!("usage: muse-au-verify PLUGIN.component FACTORY SUBTYPE MANUFACTURER");
            std::process::exit(2);
        }
        let mut report = Report::new(true);
        let Some(desc) = description(&args[3], &args[4]) else {
            eprintln!("subtype and manufacturer must each be four printable characters");
            std::process::exit(2);
        };
        unsafe {
            let lib = match library(Path::new(&args[1])) {
                Ok(l) => l,
                Err(e) => {
                    report.check("load", false, e);
                    std::process::exit(if report.finish() { 0 } else { 1 });
                }
            };
            let c_lib = CString::new(lib.to_string_lossy().to_string()).unwrap();
            let handle = dlopen(c_lib.as_ptr(), RTLD_NOW);
            if handle.is_null() {
                let why = CStr::from_ptr(dlerror()).to_string_lossy().to_string();
                report.check("load", false, format!("dlopen failed: {}", why));
                std::process::exit(if report.finish() { 0 } else { 1 });
            }
            report.check("load", true, lib.display().to_string());
            let name = CString::new(args[2].clone()).unwrap();
            let symbol = dlsym(handle, name.as_ptr());
            if symbol.is_null() {
                report.check("factory_symbol", false, format!("the library does not export {}", args[2]));
                std::process::exit(if report.finish() { 0 } else { 1 });
            }
            report.check("factory_symbol", true, format!("{} is exported", args[2]));
            let factory: FactoryFunction = std::mem::transmute::<*mut c_void, FactoryFunction>(symbol);
            run(factory, desc, &mut report);
            let closed = dlclose(handle) == 0;
            report.check("unload", closed, "dlclose completed");
        }
        std::process::exit(if report.finish() { 0 } else { 1 });
    }
}

#[cfg(target_os = "macos")]
fn main() {
    mac::main();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("muse-au-verify runs on macOS only");
    std::process::exit(2);
}
