//! Dump the Lua → JS transpiler's output for a file.
//!
//! The browser runtimes compile game scripts through
//! `bridge::adapters::js::transpile_lua_to_js`, so a construct the transpiler
//! mishandles only shows up as a JS syntax error in a browser console. This
//! example makes that loop checkable from the terminal:
//!
//! ```text
//! cargo run -p jengine-core --features js-adapter-host \
//!     --example transpile_lua -- path/to/script.lua > out.js
//! node --check out.js
//! ```
//!
//! Requires the `js-adapter-host` feature (the adapter that owns the
//! transpiler is gated on it).

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: transpile_lua <script.lua>");
        std::process::exit(2);
    };
    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("reading {path}: {e}");
            std::process::exit(1);
        }
    };
    print!(
        "{}",
        jengine_core::scripting::bridge::adapters::js::transpile_lua_to_js(&source)
    );
}
