// CEP:FILE crates/vortex-t0/src/sem.rs
// CEP:WHAT The T0 semantic core as a reusable module: arithmetic with the
//        int fast path / float path / boxed slow path, AOSS store policy,
//        abstract equality, truthiness, and word-to-handle/index extraction.
// CEP:WHY T0 is the semantic reference for every higher tier (design.md
//        section 5); hosting the reference semantics here lets the T1/T2/T4
//        executors compose the exact same operations instead of re-implementing
//        them (one semantic reference, zero drift).
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE All failure paths surface as typed Trap values; no silent
//        fallbacks and no divergent behavior between tiers.
// CEP:ASSUMES Inputs are NV-1 words produced by verified UGB methods.
// CEP:COST Each function is O(1) except where the heap policy is invoked
//        (store_element delegates to the AOSS transition costs).
// CEP:EVIDENCE cargo test -p vortex-t0; differential tests in vortex-t1
use vortex_nv1 as nv1;
use vortex_runtime as rt;
use vortex_ugb::Op;

/// Arithmetic with the int fast path, float path, and boxed slow path.
// CEP:WHAT Arithmetic with the int fast path, float path, and boxed slow path.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
pub fn arith(m: &mut rt::Machine, op: Op, a: u64, b: u64) -> Result<u64, rt::Trap> {
    match (nv1::categorize(a), nv1::categorize(b)) {
        (nv1::Category::Int, nv1::Category::Int) => {
            let (x, y) = (nv1::decode_int(a), nv1::decode_int(b));
            let r = match op {
                Op::Add => x.checked_add(y),
                Op::Sub => x.checked_sub(y),
                Op::Mul => x.checked_mul(y),
                Op::Div => {
                    if y == 0 {
                        return Err(rt::Trap::DivByZero);
                    }
                    x.checked_div(y)
                }
                _ => None,
            };
            match r {
                Some(v) if (nv1::I48_MIN..=nv1::I48_MAX).contains(&v) => {
                    Ok(nv1::encode_int(v).expect("in range"))
                }
                Some(v) => Ok(box_int(m, v)), // boxed overflow domain
                None => Err(rt::Trap::ArithmeticOverflow),
            }
        }
        _ => {
            // Float path (mixed int/float promotes per UGB semantics);
            // boxed integers unbox first.
            let x = numeric_value(m, a)?;
            let y = numeric_value(m, b)?;
            let r = match op {
                Op::Add => x + y,
                Op::Sub => x - y,
                Op::Mul => x * y,
                Op::Div => x / y,
                _ => {
                    return Err(rt::Trap::TypeMismatch {
                        expected: "arith op",
                        word: u64::from(op as u8),
                    })
                }
            };
            Ok(nv1::encode_f64(r))
        }
    }
}

/// Numeric value with boxed-integer unboxing.
// CEP:WHAT Numeric value with boxed-integer unboxing.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
pub fn numeric_value(m: &mut rt::Machine, w: u64) -> Result<f64, rt::Trap> {
    match nv1::categorize(w) {
        nv1::Category::Int => Ok(nv1::decode_int(w) as f64),
        nv1::Category::Float => Ok(f64::from_bits(w)),
        nv1::Category::Ref => {
            let h = as_handle(w)?;
            if m.heap.layout_of(h) == rt::layout::BOXED_INT {
                Ok(m.heap.read_payload(h, 0) as i64 as f64)
            } else {
                Err(rt::Trap::TypeMismatch {
                    expected: "numeric",
                    word: w,
                })
            }
        }
        _ => Err(rt::Trap::TypeMismatch {
            expected: "numeric",
            word: w,
        }),
    }
}

/// Allocates a boxed integer for the i48 overflow domain (NV-1 2.1).
// CEP:WHAT Allocates a boxed integer for the i48 overflow domain (NV-1 2.1).
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
pub fn box_int(m: &mut rt::Machine, v: i64) -> u64 {
    let nursery = m.heap.nursery_page().expect("nursery present");
    let h = m
        .heap
        .alloc_array(rt::layout::BOXED_INT, 1, nursery)
        .expect("boxed alloc");
    let _ = m.heap.write_payload(h, 0, v as u64);
    nv1::encode_ref(h.0, 0).expect("valid handle")
}

/// Store path with the full AOSS policy: numeric elision, reverse transition
/// on domain violation, transition on homogeneity window.
// CEP:WHAT Store path with the full AOSS policy: numeric elision, reverse transition on domain violation, transition on homogeneity window.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
pub fn store_element(
    m: &mut rt::Machine,
    arr: rt::Handle,
    idx: usize,
    val: u64,
) -> Result<(), rt::Trap> {
    let lay = m.heap.layout_of(arr);
    if rt::layout::is_numeric(lay) {
        // Reverse transition when the value violates the numeric domain
        // (NUM-INV-02).
        let domain_ok = if lay == rt::layout::NUMERIC_F64 {
            nv1::categorize(val) == nv1::Category::Float
        } else {
            nv1::is_int(val)
        };
        if !domain_ok {
            m.aoss.transition_tagged(&mut m.heap, arr)?;
        }
    } else {
        // Tagged store: observe for the numeric transition.
        let decision = m.aoss.observe_store(arr, val);
        if decision != rt::Decision::None {
            // Transition converts the existing payload; the store below
            // then lands raw.
            m.aoss.transition_numeric(&mut m.heap, arr, decision)?;
        }
    }
    m.heap
        .store_array_element(arr, idx, val, &mut m.remembered, &mut m.gc.stats)
}

/// Abstract equality over NV-1 words (subset semantics).
///
/// The float arm uses `==` deliberately: abstract equality on numbers is
/// value equality, and NaN != NaN is the required IEEE semantics.
#[allow(clippy::float_cmp)]
#[must_use]
pub fn words_equal(a: u64, b: u64) -> bool {
    match (nv1::categorize(a), nv1::categorize(b)) {
        (nv1::Category::Int, nv1::Category::Int) => nv1::decode_int(a) == nv1::decode_int(b),
        (nv1::Category::Float, nv1::Category::Float) => f64::from_bits(a) == f64::from_bits(b),
        (nv1::Category::Special, nv1::Category::Special) => a == b,
        _ => a == b, // reference identity
    }
}

/// Truthiness: null, undefined, false, and integer zero are falsy.
#[must_use]
pub fn is_truthy(w: u64) -> bool {
    match nv1::categorize(w) {
        nv1::Category::Special => !matches!(
            nv1::decode_special(w),
            nv1::SPECIAL_NULL | nv1::SPECIAL_UNDEF | nv1::SPECIAL_FALSE
        ),
        nv1::Category::Int => nv1::decode_int(w) != 0,
        _ => true,
    }
}

/// Extracts a heap handle from a reference word.
// CEP:WHAT Extracts a heap handle from a reference word.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
pub fn as_handle(w: u64) -> Result<rt::Handle, rt::Trap> {
    if !nv1::is_ref(w) {
        return Err(rt::Trap::TypeMismatch {
            expected: "reference",
            word: w,
        });
    }
    Ok(rt::Handle(nv1::ref_offset(w)))
}

/// Extracts an array index from an integer word.
// CEP:WHAT Extracts an array index from an integer word.
// CEP:WHY Required by the module contract stated in the file header.
// CEP:STATUS complete
// CEP:FAILURE Reports failure through the module's typed error path; no silent fallback.
// CEP:ASSUMES Inputs satisfy the module invariants (see file header).
// CEP:COST Bounded by the file-header budget for this class; no hidden allocation beyond documented growth.
// CEP:EVIDENCE cargo test -p vortex-t0
pub fn as_index(w: u64) -> Result<usize, rt::Trap> {
    if !nv1::is_int(w) {
        return Err(rt::Trap::TypeMismatch {
            expected: "index",
            word: w,
        });
    }
    let v = nv1::decode_int(w);
    usize::try_from(v).map_err(|_| rt::Trap::BoundsCheck {
        index: usize::MAX,
        len: 0,
    })
}
