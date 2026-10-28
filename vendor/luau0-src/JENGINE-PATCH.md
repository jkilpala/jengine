# jEngine's patch to luau0-src 0.12.3+luau663

Luau, built so a script's numbers are the same on every platform
(docs/determinism-proposal.md, phase 3). The workspace and every crate the
builder generates `[patch]` `luau0-src` to this copy
(`crates/jengine-builder/src/packages.rs`, `luau_patch`).

Three things made a Lua script compute different numbers on different
machines:

1. **The platform's maths library.** `math.sin`, `math.exp`, `math.pow`, the
   `^` operator, `math.log(x, base)`, `vector.angle` - and the compiler's
   constant folding of all of them - called the C library's `sin`, `pow`
   and so on, whose last bit differs between Windows, macOS, Linux and
   Android. They call through `luau_jengine_math`, a table of function
   pointers (`luau/Common/include/Luau/JEngineMath.h`, defined in
   `luau/Ast/src/JEngineMath.cpp`), which `jengine_core::scripting::script_math`
   fills with Rust's `libm` before it compiles a script. The browsers'
   Fengari calls the same Rust functions in the engine's wasm module, so a
   number is the same natively and in the browser.
2. **Fused multiply-add.** Clang's default `-ffp-contract=on` fused
   `a - floor(a / b) * b` (`%`) into one `fmsub` on ARM64 - 41 fused
   instructions in the VM - so a Mac or a phone disagreed with x86 and the
   browser. `src/lib.rs` builds with `-ffp-contract=off`, and `luaconf.h`
   no longer turns MSVC's precise floating point off around `%` and `//`.
3. **`^` had two rules.** A constant exponent of 2, 0.5 or 3 was computed as
   `a * a`, `sqrt(a)` or `a * a * a`; any other exponent, or the same ones
   held in a variable, went to `pow`. `luau_jengine_pow` is the one rule,
   used by both opcodes, `math.pow` and the folding - and by Fengari.

Also: `math.atan(y, x)` takes Lua 5.3's second argument (the fast call
leaves a two-argument call to the library function), so a script can use
either spelling in both of the engine's Lua dialects.

Every changed line is marked `jEngine patch`. To move to a new luau0-src:
copy its crate here, run `grep -rn "jEngine patch"` against this copy and
re-apply each site (the call sites in `lbuiltins.cpp`, `lmathlib.cpp`,
`lveclib.cpp`, `lvmexecute.cpp`, `BuiltinFolding.cpp`,
`ConstantFolding.cpp`; `lnumutils.h`; `luaconf.h`; `src/lib.rs`), and check
`objdump -d` of `libluauvm.a` on an ARM64 machine shows no `fmadd` / `fmsub`
(`tools/determinism/check_luau_build.sh`).
