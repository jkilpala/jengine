# rapier2d 0.22.0, patched for jEngine

This is rapier2d 0.22.0 from crates.io with one kind of change: **the maths
calls rapier makes on `f32` directly go through `libm`**, so a step is the same
bits on every platform (docs/determinism-proposal.md).

The workspace already turns on rapier's `enhanced-determinism`, which sends
nalgebra's and parry's maths (through simba's `libm_force`) to `libm` and stops
rapier flushing denormals to zero on x86. It cannot reach code that calls an
`f32` method itself - `x.sin()` on an `f32` is the platform's whatever simba
says. Found by listing the system maths symbols a test binary imports
(`nm -u`): after the feature, only these were left, and a derby match from one
seed came out different on Windows than on macOS, Linux and wasm.

- `src/geometry/broad_phase_multi_sap/sap_utils.rs`, `layer_containing_aabb`:
  `log(base)`, rounded to a layer. Run every step; near a half, a different
  last bit put a body in a different layer, reordering the broad phase's
  pairs.
- `src/dynamics/solver/joint_constraint/joint_generic_constraint_builder.rs`:
  `sin` of half a joint's angle and limits (eight calls).
- `src/dynamics/joint/revolute_joint.rs`: `asin` of the angle error.
- `src/geometry/broad_phase_multi_sap/sap_utils.rs`, `region_width`, and
  `src/dynamics/rigid_body_components.rs`, `kinetic_energy`: `powi`. Not a
  call into the maths library on macOS or Linux, where LLVM sends it to
  compiler-rt's `__powisf2` - so `nm -u` there cannot see it - but **MSVC has
  no `__powisf2`**, and on Windows `powi` becomes the CRT's `powf`. Found by
  `dumpbin /imports` on the Windows test binary (2026-10-27). Written out as
  compiler-rt's loop, so every platform keeps the bits the Mac pinned.

Each is marked "jEngine patch" or reads `libm::`. `Cargo.toml` gains `libm`.
To move to a newer rapier: take its source, look for `f32` maths methods the
same way (`nm -u` on `jengine-determinism`'s test binary must list none but
`fmod`/`fmodf`, which are exact), and patch what is left.
