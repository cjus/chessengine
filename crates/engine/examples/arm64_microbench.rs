//! Standalone, dependency-free microbenchmarks for Apple Silicon investigations.
//! Run with: cargo run --release -p cb2026-engine --example arm64_microbench
//! Use RUSTFLAGS="-C target-cpu=native" only as a separate, recorded variant.
use std::hint::black_box;
use std::time::Instant;
use cb2026_chess::{Bitboard, Position, Square, attacks};
use cb2026_engine::builtin_network;
use cb2026_nnue::Changes;

const FENS: [&str; 4] = [
    "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
];

fn measure(mut f: impl FnMut(usize), rounds: usize, iterations: usize) {
    for i in 0..iterations / 10 { f(i); }
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let start = Instant::now();
        for i in 0..iterations { f(i); }
        samples.push(start.elapsed().as_secs_f64() * 1e9 / iterations as f64);
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[rounds / 2];
    println!("  ns/op median={median:.2} min={:.2} max={:.2} iterations={iterations} rounds={rounds}", samples[0], samples[rounds - 1]);
}

fn main() {
    let rounds = 11;
    let positions: Vec<_> = FENS.iter().map(|f| Position::from_fen(f).expect("valid FEN")).collect();
    let net = builtin_network();
    eprintln!("network: {}", net.summary());
    eprintln!("target: {} / {} ; use a quiet, thermally stable machine", std::env::consts::ARCH, std::env::consts::OS);

    println!("bitboard count/scan");
    measure(|i| {
        let x = black_box(0x8100_2400_0000_0081u64.rotate_left((i & 63) as u32));
        black_box(Bitboard(x).count());
        black_box(Bitboard(x).first());
        black_box(Bitboard(x).squares().count());
    }, rounds, 100_000);

    println!("rook/bishop magic attacks");
    measure(|i| {
        let sq = Square::from_index(i & 63).unwrap();
        let occupied = Bitboard(black_box(0x0081_0024_0018_8100u64.rotate_left((i & 63) as u32)));
        black_box(attacks::rook_attacks(sq, occupied));
        black_box(attacks::bishop_attacks(sq, occupied));
    }, rounds, 100_000);

    println!("NNUE full refresh + evaluation");
    measure(|i| {
        let pos = black_box(&positions[i % positions.len()]);
        let acc = net.accumulator(pos);
        black_box(net.evaluate(&acc, pos.side_to_move()));
    }, rounds, 10_000);

    // Prepare a legal move and its corresponding changes; update_into must be exact.
    let pos = Position::startpos();
    let mv = pos.legal_moves()[0];
    let changes = Changes::of(&pos, mv);
    let parent = net.accumulator(&pos);
    let mut next = pos.clone();
    next.make_move(mv);
    let expected = net.accumulator(&next);
    let mut child = net.empty_accumulator();
    net.update_into(&parent, &mut child, &changes);
    assert_eq!(child, expected, "incremental accumulator must match full refresh");
    assert_eq!(net.evaluate(&child, next.side_to_move()), net.evaluate_position(&next));
    println!("NNUE incremental update_into only");
    measure(|_| {
        net.update_into(black_box(&parent), black_box(&mut child), black_box(&changes));
        black_box(&child);
    }, rounds, 30_000);

    println!("NNUE evaluation only (precomputed accumulator)");
    measure(|_| {
        black_box(net.evaluate(black_box(&expected), black_box(next.side_to_move())));
    }, rounds, 100_000);
}
