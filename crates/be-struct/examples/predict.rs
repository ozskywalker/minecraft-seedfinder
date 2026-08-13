//! Small CLI to validate `be-struct` placement against an observed structure
//! position (e.g. from a real world or `/locate`).
//!
//! Usage:
//!   cargo run -p be-struct --example predict -- \
//!     --seed <seed> --structure <id> --x <block_x> --z <block_z> [--table <version>]
//!
//! It backs out the structure region from the observed block position, predicts the
//! position for that region with be-struct, and prints observed vs predicted plus
//! the offset. Used for Phase 0 ground-truth validation.

use be_struct::{region_of_block, structure_block_pos, Version};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    };

    // Minecraft world seeds are signed 64-bit. be-struct stores the seed as raw u64
    // bits; the low-32-bit structure math is unaffected by the sign, so we parse the
    // user-facing signed value and reinterpret its bits as u64.
    let seed: u64 = (get("--seed")
        .expect("--seed")
        .parse::<i64>()
        .expect("seed parses as signed i64")) as u64;
    let structure = get("--structure").expect("--structure").to_string();
    let bx: i64 = get("--x").expect("--x").parse().expect("x");
    let bz: i64 = get("--z").expect("--z").parse().expect("z");
    let table = get("--table")
        .map(String::from)
        .unwrap_or_else(|| "1.21.40".into());

    let version = Version::builtin_1_21_40();
    let params = version.structures.get(&structure).unwrap_or_else(|| {
        eprintln!("structure {structure} not in table {table}");
        std::process::exit(1)
    });

    let reg_x = region_of_block(bx, params.spacing);
    let reg_z = region_of_block(bz, params.spacing);
    let (px, pz) = structure_block_pos(
        seed,
        reg_x,
        reg_z,
        params.salt,
        params.spacing,
        params.chunk_range,
        params.distribution(),
    );

    let dx = px - bx;
    let dz = pz - bz;
    let dist = ((dx * dx + dz * dz) as f64).sqrt();

    println!("structure : {structure}");
    println!("version   : {table}");
    println!(
        "region    : ({reg_x}, {reg_z})   [spacing={}]",
        params.spacing
    );
    println!("observed  : ({bx}, {bz})");
    println!("predicted : ({px}, {pz})");
    println!("offset    : dx={dx}, dz={dz}, dist={dist:.1}");
    println!("EXACT     : {}", dx == 0 && dz == 0);
}
