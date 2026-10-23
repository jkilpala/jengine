//! Is interning asset ids worth it?
//!
//! `docs/architecture-debt.md` #1 says identity being `String` costs twice:
//! an allocation per drawable per frame, and a string comparison inside the
//! render sort. The allocation half is fixed — the render paths borrow now.
//! This measures the half that is left, before committing to a change that
//! touches every component, the scene format and every render path.
//!
//! `#[ignore]`d: timing. Run with
//! `cargo test -p jengine-core --release --test sort_key_cost -- --ignored --nocapture`

use std::time::Instant;

/// Sort keys shaped like the renderer's: resolved layer, material, texture.
fn build_str_keys(n: usize, distinct_tex: usize) -> Vec<(i32, Option<&'static str>, &'static str)> {
    const TEX: [&str; 8] = [
        "__white",
        "sprites/white32.png",
        "sprites/player_idle.png",
        "sprites/tileset_dungeon.png",
        "sprites/particles_soft.png",
        "__circle",
        "sprites/ui_panel_9slice.png",
        "sprites/enemy_walk_sheet.png",
    ];
    const MAT: [Option<&str>; 3] = [None, Some("lit_sprite"), Some("water_graph")];
    (0..n)
        .map(|i| {
            // Most sprites share a z_order — that is the case that actually
            // exercises the tie-breakers, and it is what scene 61 looks like.
            let z = if i % 16 == 0 { (i % 5) as i32 } else { 5 };
            (z, MAT[i % 3], TEX[i % distinct_tex.min(8)])
        })
        .collect()
}

/// The same keys with ids interned to integers.
fn build_int_keys(n: usize, distinct_tex: usize) -> Vec<(i32, u32, u32)> {
    (0..n)
        .map(|i| {
            let z = if i % 16 == 0 { (i % 5) as i32 } else { 5 };
            (z, (i % 3) as u32, (i % distinct_tex.min(8)) as u32)
        })
        .collect()
}

fn median_ms(mut f: impl FnMut()) -> f64 {
    let mut s: Vec<f64> = (0..9)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[s.len() / 2]
}

#[test]
#[ignore = "timing-sensitive; run with --release"]
fn compare_string_and_interned_sort_keys() {
    println!();
    println!("{:>8}  {:>10}  {:>10}  {:>9}", "sprites", "string ms", "int ms", "saved ms");
    println!("{:->8}  {:->10}  {:->10}  {:->9}", "", "", "", "");
    for n in [2_000usize, 8_000, 16_000] {
        let s_keys = build_str_keys(n, 4);
        let i_keys = build_int_keys(n, 4);

        let s_ms = median_ms(|| {
            let mut v = s_keys.clone();
            v.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)).then_with(|| a.2.cmp(b.2)));
            std::hint::black_box(&v);
        });
        let i_ms = median_ms(|| {
            let mut v = i_keys.clone();
            v.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));
            std::hint::black_box(&v);
        });
        println!("{n:>8}  {s_ms:>10.3}  {i_ms:>10.3}  {:>9.3}", s_ms - i_ms);
    }
    println!();
    println!("Against a ~16 ms frame at 16k sprites. Interning also removes a");
    println!("String from every sprite component, which is memory, not time.");
}
