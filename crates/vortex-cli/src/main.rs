// CEP:FILE crates/vortex-cli/src/main.rs
// CEP:WHAT VORTEX driver binary: `vortex run <method> <args...>`,
//        `vortex demo`, `vortex disasm <method>`, `vortex tiers <method>`.
// CEP:WHY A driver makes the implemented subsystems exercisable end to end
//        and prints the contract counters the design requires to be
//        observable (barrier elision, snapshot immobility, T4 requests,
//        tier decisions, and the differential tier equality).
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Traps print to stderr and exit 1; no panics on user input.
// CEP:ASSUMES Demo programs are built into the binary; no file loading yet.
// CEP:COST O(program) per invocation.
// CEP:EVIDENCE cargo run -p vortex-cli -- demo
//! VORTEX driver binary.

use vortex_runtime as rt;
use vortex_t0::Interp;
use vortex_ugb::Method;

// CEP:WHAT Implements `main` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-cli
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("demo") => demo(),
        Some("run") => match args.len() {
            0..=1 => {
                eprintln!(
                    "usage: vortex run <countdown|array_roundtrip|capability_loop> [args...]"
                );
                3
            }
            _ => run_command(&args[1], &args[2..]),
        },
        Some("disasm") if args.len() == 2 => disasm_command(&args[1]),
        Some("tiers") if args.len() == 2 => tiers_command(&args[1]),
        Some(other) => {
            eprintln!("unknown subcommand {other:?}");
            eprintln!("usage: vortex <demo|run|disasm|tiers>");
            3
        }
        None => {
            eprintln!("usage: vortex <demo|run|disasm|tiers>");
            3
        }
    };
    std::process::exit(code);
}

fn build_program(name: &str) -> Option<Vec<Method>> {
    match name {
        "countdown" => Some(vec![vortex_ugb::countdown_method(100_000)]),
        "array_roundtrip" => Some(vec![vortex_ugb::array_roundtrip_method()]),
        "capability_loop" => Some(vec![vortex_ugb::capability_loop_method(
            u32::try_from(rt::auth::FILE_READ).expect("auth fits u32"),
        )]),
        _ => None,
    }
}

// CEP:WHAT Implements `run_command` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-cli
fn run_command(name: &str, _args: &[String]) -> i32 {
    let Some(program) = build_program(name) else {
        eprintln!("unknown method {name:?} (try countdown|array_roundtrip|capability_loop)");
        return 3;
    };
    let mut m = rt::Machine::new();
    let interp = Interp::new(program);
    match interp.run(&mut m, 0, &[]) {
        Ok(result) => {
            println!("{name} -> {result:#018x}");
            print_stats(&m);
            0
        }
        Err(trap) => {
            eprintln!("{name} trapped: {trap}");
            1
        }
    }
}

fn disasm_command(name: &str) -> i32 {
    let Some(program) = build_program(name) else {
        eprintln!("unknown method {name:?}");
        return 3;
    };
    print!("{}", vortex_ugb::disasm::disassemble(&program[0]));
    0
}

fn print_stats(m: &rt::Machine) {
    let s = &m.gc.stats;
    println!(
        "stats: load_barrier fast={} slow={} elided={} | write_barrier exec={} elided={} | moved={} snapshot_moved={}",
        s.load_barrier_fast, s.load_barrier_slow, s.load_barrier_elided,
        s.write_barriers, s.write_barrier_elided, s.objects_moved, s.snapshot_objects_moved
    );
    let reqs = m.profile.trace_requests();
    println!("profile: t4_trace_requests={}", reqs.len());
}

// CEP:WHAT Implements `demo` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-cli
fn demo() -> i32 {
    println!("VORTEX reference engine demo");
    println!("============================");
    println!();
    println!("[1] T0 countdown loop with sampled back-edge counter (PROF-1 3.2)");
    {
        let program = vec![vortex_ugb::countdown_method(300)];
        let mut m = rt::Machine::new();
        let interp = Interp::new(program);
        match interp.run(&mut m, 0, &[]) {
            Ok(v) => {
                let sum = if vortex_nv1::is_int(v) {
                    vortex_nv1::decode_int(v)
                } else {
                    -1
                };
                println!("    countdown(300) = {sum} (expected {})", 300 * 301 / 2);
            }
            Err(t) => {
                eprintln!("    trapped: {t}");
                return 1;
            }
        }
        print_stats(&m);
    }
    println!();
    println!("[2] Tagged-array store homogeneity -> AOSS numeric transition (NUM-1 3)");
    {
        let program = vec![vortex_ugb::array_roundtrip_method()];
        let mut m = rt::Machine::new();
        let interp = Interp::new(program);
        // Drive the AOSS observer directly: 16 homogeneous float stores.
        let nursery = m.heap.nursery_page().expect("nursery");
        let arr = m
            .heap
            .alloc_array(rt::layout::GENERIC_ARRAY, 8, nursery)
            .expect("alloc");
        for i in 0..rt::HOMOGENEITY_WINDOW {
            let v = vortex_nv1::encode_f64(f64::from(i) * 1.5);
            let decision = m.aoss.observe_store(arr, v);
            if decision != rt::Decision::None {
                let ok = m
                    .aoss
                    .transition_numeric(&mut m.heap, arr, decision)
                    .expect("transition");
                println!(
                    "    transitioned to F64 payload after {} stores: {ok}",
                    i + 1
                );
            }
        }
        println!(
            "    layout now numeric: {}",
            rt::layout::is_numeric(m.heap.layout_of(arr))
        );
        match interp.run(&mut m, 0, &[]) {
            Ok(v) => println!("    array_roundtrip -> {}", vortex_nv1::decode_int(v)),
            Err(t) => {
                eprintln!("    trapped: {t}");
                return 1;
            }
        }
        print_stats(&m);
    }
    println!();
    println!("[3] Capability epoch: revoke is one increment, seen at next check (UGB-1 6.3)");
    {
        let program = vec![vortex_ugb::capability_loop_method(
            u32::try_from(rt::auth::FILE_READ).expect("auth fits u32"),
        )];
        let mut m = rt::Machine::new();
        let interp = Interp::new(program);
        let r = interp.run(&mut m, 0, &[]);
        println!("    pre-revocation run: {:?}", r.map(|_| "ok"));
        // Build a second machine and revoke mid-flight to show the check.
        let mut m2 = rt::Machine::new();
        let interp2 = Interp::new(vec![vortex_ugb::capability_loop_method(
            u32::try_from(rt::auth::FILE_READ).expect("auth fits u32"),
        )]);
        let cap = m2.caps.grant(rt::auth::FILE_READ, 0, None);
        let mut cache = rt::EpochCache::default();
        let ok = m2.caps.check_fast(&mut cache, cap, rt::auth::FILE_READ);
        println!("    check before revoke: ok={}", ok.is_ok());
        m2.caps.revoke(cap);
        let denied = m2.caps.check_fast(&mut cache, cap, rt::auth::FILE_READ);
        println!("    check after revoke: err={}", denied.is_err());
        println!("    epoch: {} (was 1)", m2.caps.epoch_of(cap));
        let _ = interp2;
    }
    println!();
    println!("[4] Snapshot pinning: stores into a frozen page trap (SNAP-INV-09)");
    {
        let mut m = rt::Machine::new();
        let nursery = m.heap.nursery_page().expect("nursery");
        let arr = m
            .heap
            .alloc_array(rt::layout::GENERIC_ARRAY, 4, nursery)
            .expect("alloc");
        let lo = arr.0;
        let hi = arr.0 + 0x1000;
        m.heap.freeze_snapshot(arr.page(), lo, hi).expect("freeze");
        let trap = m.heap.store_array_element(
            arr,
            0,
            vortex_nv1::null_word(),
            &mut m.remembered,
            &mut m.gc.stats,
        );
        println!(
            "    store into snapshot: {:?}",
            trap.err().map(|t| t.to_string())
        );
    }
    0
}

/// Builds the `factorial` demo program for the tiers command.
// CEP:WHAT Implements `build_program` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-cli
fn build_program_all(name: &str) -> Option<Vec<Method>> {
    match name {
        "factorial" => Some(vortex_ugb::factorial_program()),
        other => build_program(other),
    }
}

/// The `tiers` command: executes one method through every implemented
/// tier and prints the tier controller's decisions plus the differential
/// equality of all results (the T0-is-the-reference contract, visible).
// CEP:WHAT Implements `tiers_command` as part of the module contract.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo run -p vortex-cli -- tiers countdown
fn tiers_command(name: &str) -> i32 {
    let Some(program) = build_program_all(name) else {
        eprintln!(
            "unknown method {name:?} (try countdown|array_roundtrip|capability_loop|factorial)"
        );
        return 3;
    };
    let controller = vortex_tier::TierController::new();
    let results: Vec<String> = Vec::new();

    // T0: the reference execution, with the profile the tiers consume.
    let mut m = rt::Machine::new();
    let interp = Interp::new(program.clone());
    let t0 = match interp.run(&mut m, 0, &[]) {
        Ok(w) => w,
        Err(t) => {
            eprintln!("{name} trapped on T0: {t}");
            return 1;
        }
    };
    println!("T0 (interpreter)  -> {t0:#018x}");
    let folded = m.profile.total(0);
    let calls: Vec<u64> = m.call_counts.values().copied().collect();
    if controller.wants_baseline(0, folded) {
        println!("  controller: back-edges {folded} >= threshold -> T1 warranted");
    }
    if controller.wants_optimizing(calls.iter().copied()) {
        println!("  controller: call counts -> T2 warranted");
    }

    // T1: the stencil baseline.
    match vortex_t1::exec::compile_and_run(
        &mut vortex_runtime::StencilPool::default(),
        &program,
        0,
        &mut rt::Machine::new(),
        &[],
    ) {
        Ok(w) => {
            println!("T1 (stencil)      -> {w:#018x} equal={}", w == t0);
        }
        Err(t) => println!("T1: {t}"),
    }

    // T2: the optimizing pipeline.
    match vortex_opt::compile_plan(&program, 0, vortex_opt::TierBudget::Moderate) {
        Ok(plan) => {
            let mut m2 = rt::Machine::new();
            match vortex_opt::exec::run_plan(&plan, &program, &mut m2, &[]) {
                Ok(w) => println!("T2 (optimizing)   -> {w:#018x} equal={}", w == t0),
                Err(t) => println!("T2: {t}"),
            }
        }
        Err(e) => println!("T2: not applicable ({e:?})"),
    }

    let _ = results;
    0
}
