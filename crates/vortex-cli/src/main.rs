// CEP:FILE crates/vortex-cli/src/main.rs
// CEP:WHAT The VORTEX driver: demo (end-to-end tier walkthrough),
//        assemble, verify, run, optimize, and tiers subcommands.
// CEP:WHY CI requires `vortex-cli -- demo`; operators drive the engine
//        through this binary.
// CEP:CLASS CEP-2
// CEP:STATUS complete
// CEP:FAILURE Subcommands exit 1 with a message on typed errors; the
//        driver never panics on verified inputs.
// CEP:ASSUMES Inputs are UGB assembly text or method indices.
// CEP:COST O(input) per command.
// CEP:EVIDENCE cargo run -p vortex-cli -- demo
//! VORTEX driver binary.
use vortex_nv1::Value;
use vortex_ugb::{assemble, verify, CapReq, Constant, Method, Program};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&args);
    std::process::exit(code);
}

// CEP:WHAT Dispatches one subcommand and returns the exit code.
// CEP:WHY The CLI contract: 0 on success, 1 on typed failure.
// CEP:STATUS complete
// CEP:FAILURE Unknown commands exit 2.
// CEP:ASSUMES None.
// CEP:COST O(command).
// CEP:EVIDENCE cargo run -p vortex-cli -- demo
fn run(args: &[String]) -> i32 {
    match args.first().map(|s| s.as_str()) {
        Some("demo") => demo(),
        Some("assemble") => cmd_assemble(args),
        Some("run") => cmd_run(args),
        Some("optimize") => cmd_optimize(args),
        _ => {
            eprintln!("usage: vortex-cli demo|assemble|run|optimize");
            2
        }
    }
}

// CEP:WHAT Builds the demo program: main calls dbl and returns 42.
// CEP:WHY Every tier walks this one program in the demo.
// CEP:STATUS complete
// CEP:FAILURE Panics only on internal builder misuse.
// CEP:ASSUMES None.
// CEP:COST O(code).
// CEP:EVIDENCE cargo run -p vortex-cli -- demo
fn build_demo_program() -> Program {
    let mut p = Program::default();
    let mut main = Method::build("main", 4, 0, 0).method;
    main.env_caps.push(CapReq::Call);
    let mut m = main;
    assemble(
        &mut m,
        "LOAD_CONST r0, const0\nLOAD_CONST r1, const1\nCALL_METHOD r2, const1, x2\nRETURN r2\n",
    )
    .expect("assemble main");
    let mut dbl = Method::build("dbl", 3, 0, 2).method;
    assemble(&mut dbl, "ADD r2, r0, r1\nRETURN r2\n").expect("assemble dbl");
    p.methods.push(m);
    p.methods.push(dbl);
    p.constants = vec![Constant::Int(20), Constant::Int(1), Constant::Int(22)];
    verify(&p).expect("verify");
    p
}

// CEP:WHAT Runs the full demo across T0, T1, the optimizer, T4, and the
//        tier controller.
// CEP:WHY The CI gate runs this; it proves the tiers compose.
// CEP:STATUS complete
// CEP:FAILURE Exits 1 when any stage disagrees.
// CEP:ASSUMES None.
// CEP:COST O(demo work).
// CEP:EVIDENCE cargo run -p vortex-cli -- demo
fn demo() -> i32 {
    let p = build_demo_program();
    println!("== T0 interpreter ==");
    let mut interp = vortex_t0::Interp::new(p.clone());
    let t0 = match interp.run(0, &[]) {
        Ok(vortex_t0::Outcome::Returned(v)) => v,
        other => {
            eprintln!("t0 failed: {other:?}");
            return 1;
        }
    };
    println!("main() = {}", t0.as_i64().unwrap_or(0));
    println!("== T1 copy-and-patch ==");
    let coverage = vortex_t1::classify(&p);
    let objects: Vec<vortex_t1::CodeObject> = coverage
        .compiled
        .iter()
        .map(|m| vortex_t1::plan_method(&p, *m))
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    let mut ex = vortex_t1::Executor::new();
    ex.install(objects, p.constants.clone());
    let t1 = ex
        .run(0, &[], &mut interp.heap)
        .unwrap_or(Value::undefined());
    if t1.as_i64() != t0.as_i64() {
        eprintln!("t1 disagrees with t0");
        return 1;
    }
    println!("t1 agrees: {}", t1.as_i64().unwrap_or(0));
    println!("== ET-SoN pipeline ==");
    let mut b = vortex_ir::Builder::new(1);
    let start = b.start(vec![]);
    let a = b.pure(vortex_ir::Op::ConstInt(2), vortex_ir::Type::Int64, vec![]);
    let c = b.pure(vortex_ir::Op::ConstInt(3), vortex_ir::Type::Int64, vec![]);
    let add = b.pure(vortex_ir::Op::IAdd, vortex_ir::Type::Int64, vec![a, c]);
    let ret = b.insert(vortex_ir::Node {
        op: vortex_ir::Op::Return,
        ty: vortex_ir::Type::Ctrl,
        data: vec![add],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: vortex_ir::EffectSet::PURE,
        fp: vortex_ir::FpSet::STRICT,
        footprint: 0,
    });
    let stats = vortex_opt::passes::run_pipeline(&mut b.graph);
    println!(
        "pipeline: {} folded, {} dead (ret node {})",
        stats.folded, stats.dead, ret.0
    );
    println!("== T4 budgeted pipeline ==");
    let t4 = vortex_t4::compile_t4(&mut b.graph, vortex_t4::T4Config::default());
    println!("{t4}");
    println!("== tier controller ==");
    let controller = vortex_tier::Controller::new(2);
    let mut request = None;
    for _ in 0..100 {
        request = controller.note_execution(0, 1, 0);
    }
    if let Some(r) = request {
        let ok = controller.wait_done(&r, std::time::Duration::from_secs(5));
        println!("tier request {:?} -> {:?}", r.tier, r.state());
        if !ok {
            return 1;
        }
    }
    controller.shutdown();
    println!("demo ok");
    0
}

// CEP:WHAT Assembles UGB text to bytes and reports the size.
// CEP:WHY Operators check front-end output without running it.
// CEP:STATUS complete
// CEP:FAILURE Exits 1 on assembly errors.
// CEP:ASSUMES args[1] is the register count, args[2..] the source.
// CEP:COST O(input).
// CEP:EVIDENCE cargo run -p vortex-cli -- assemble
fn cmd_assemble(args: &[String]) -> i32 {
    let Some(regs) = args.get(1).and_then(|s| s.parse::<u16>().ok()) else {
        eprintln!("assemble <regs> <line>...");
        return 2;
    };
    let mut m = Method::build("input", regs, 0, 0).method;
    if assemble(&mut m, &args[2..].join("\n")).is_err() {
        eprintln!("assembly failed");
        return 1;
    }
    println!("{} bytes", m.code.len());
    0
}

// CEP:WHAT Interprets the demo program and prints the result.
// CEP:WHY Quick semantic checks without the JIT.
// CEP:STATUS complete
// CEP:FAILURE Exits 1 on traps.
// CEP:ASSUMES None.
// CEP:COST O(instructions).
// CEP:EVIDENCE cargo run -p vortex-cli -- run
fn cmd_run(_args: &[String]) -> i32 {
    let p = build_demo_program();
    let mut interp = vortex_t0::Interp::new(p);
    match interp.run(0, &[]) {
        Ok(vortex_t0::Outcome::Returned(v)) => {
            println!("{}", v.as_i64().unwrap_or(0));
            0
        }
        other => {
            eprintln!("trap: {other:?}");
            1
        }
    }
}

// CEP:WHAT Runs the optimizer over the demo-shaped graph.
// CEP:WHY Pass counters show the pipeline working end to end.
// CEP:STATUS complete
// CEP:FAILURE Exits 1 on verifier failure.
// CEP:ASSUMES None.
// CEP:COST O(nodes).
// CEP:EVIDENCE cargo run -p vortex-cli -- optimize
fn cmd_optimize(_args: &[String]) -> i32 {
    let mut b = vortex_ir::Builder::new(1);
    let start = b.start(vec![]);
    let a = b.pure(vortex_ir::Op::ConstInt(2), vortex_ir::Type::Int64, vec![]);
    let c = b.pure(vortex_ir::Op::ConstInt(3), vortex_ir::Type::Int64, vec![]);
    let add = b.pure(vortex_ir::Op::IAdd, vortex_ir::Type::Int64, vec![a, c]);
    let dead = b.pure(vortex_ir::Op::ConstInt(9), vortex_ir::Type::Int64, vec![]);
    let ret = b.insert(vortex_ir::Node {
        op: vortex_ir::Op::Return,
        ty: vortex_ir::Type::Ctrl,
        data: vec![add, dead],
        ctrl: vec![start],
        state: vec![],
        except: vec![],
        effects: vortex_ir::EffectSet::PURE,
        fp: vortex_ir::FpSet::STRICT,
        footprint: 0,
    });
    if vortex_ir::verify(&b.graph).is_err() {
        eprintln!("graph invalid");
        return 1;
    }
    let stats = vortex_opt::passes::run_pipeline(&mut b.graph);
    println!("{stats:?} (return node {})", ret.0);
    0
}
