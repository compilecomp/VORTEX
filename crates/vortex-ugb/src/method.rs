// CEP:FILE crates/vortex-ugb/src/method.rs
// CEP:WHAT Method, program, constant-pool, and FFI descriptor models: the
//        container layer a UGB image is made of (UGB design 6.3, 6.7).
// CEP:WHY Front-ends emit whole programs; the verifier, interpreter, and
//        JIT consume them through these containers.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Builder methods return typed errors; accessors return Option
//        instead of panicking on out-of-range indices.
// CEP:ASSUMES Method code is the encoded byte stream of instructions.
// CEP:COST O(1) accessors; program construction is O(size of program).
// CEP:EVIDENCE cargo test -p vortex-ugb
//! Method and program containers.

use crate::ops::CapReq;
use vortex_nv1::Value;

/// Resource bounds enforced by the verifier (UGB design 6.9 item 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// Maximum encoded bytes per method.
    pub max_code_bytes: usize,
    /// Maximum registers per method.
    pub max_registers: u16,
    /// Maximum methods per program.
    pub max_methods: usize,
    /// Maximum constant-pool entries per program.
    pub max_constants: usize,
    /// Maximum FFI descriptors per program.
    pub max_ffi_descriptors: usize,
}

impl Default for Bounds {
    // CEP:WHAT Provides the default verifier bounds.
    // CEP:WHY One documented bound set keeps verifier behavior stable.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    fn default() -> Bounds {
        Bounds {
            max_code_bytes: 1 << 20,
            max_registers: 4096,
            max_methods: 1 << 16,
            max_constants: 1 << 20,
            max_ffi_descriptors: 1 << 16,
        }
    }
}

/// A constant-pool entry; NV-1 encodes the numeric forms at load time.
#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    /// Signed integer constant.
    Int(i64),
    /// Floating-point constant.
    Float(f64),
    /// Byte string constant (also used for dynamic call names).
    Bytes(Vec<u8>),
}

impl Constant {
    /// The NV-1 value form of a numeric constant.
    // CEP:WHAT Converts a numeric constant into its NV-1 value word.
    // CEP:WHY LOAD_CONST materializes constants as interpreter values.
    // CEP:STATUS complete
    // CEP:FAILURE None for byte-string constants; they are objects.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn as_value(&self) -> Option<Value> {
        match self {
            Constant::Int(n) => Value::from_i64(*n),
            Constant::Float(f) => Some(Value::from_f64(*f)),
            Constant::Bytes(_) => None,
        }
    }
}

/// A foreign-function descriptor (UGB design 6.7).
#[derive(Debug, Clone, PartialEq)]
pub struct FfiDescriptor {
    /// Stable descriptor identifier, equal to its table index.
    pub ffi_id: u32,
    /// Foreign language identifier.
    pub lang_id: u8,
    /// Foreign function name.
    pub name: String,
    /// Arity of the foreign signature.
    pub arity: u16,
    /// Additional capabilities the call requires.
    pub caps_required: Vec<CapReq>,
}

/// One method of a UGB program.
#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    /// Method name (diagnostic; calls use indices).
    pub name: String,
    /// Number of virtual registers the frame uses.
    pub reg_count: u16,
    /// Number of local slots the frame uses.
    pub local_count: u16,
    /// Number of leading registers bound to arguments.
    pub param_count: u16,
    /// Encoded instruction stream.
    pub code: Vec<u8>,
    /// Capabilities the method's environment grants on entry.
    pub env_caps: Vec<CapReq>,
}

impl Method {
    /// Starts a builder for a method with fixed frame sizes.
    // CEP:WHAT Starts a method builder with frame sizes fixed up front.
    // CEP:WHY The verifier checks register and local bounds against the
    //        declared frame; fixing sizes first keeps encoding single-pass.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn build(name: &str, reg_count: u16, local_count: u16, param_count: u16) -> MethodBuilder {
        MethodBuilder {
            method: Method {
                name: name.to_string(),
                reg_count,
                local_count,
                param_count,
                code: Vec::new(),
                env_caps: Vec::new(),
            },
        }
    }
}

/// Builder for one method's byte stream.
#[derive(Debug)]
pub struct MethodBuilder {
    /// The method under construction.
    pub method: Method,
}

impl MethodBuilder {
    /// Encodes one instruction and appends it to the code stream.
    // CEP:WHAT Encodes and appends one instruction.
    // CEP:WHY Incremental building keeps label resolution local to asm.rs.
    // CEP:STATUS complete
    // CEP:FAILURE CodecError from the instruction encoder.
    // CEP:ASSUMES None.
    // CEP:COST O(instruction bytes).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn emit(&mut self, instr: &crate::instr::Instr) -> Result<(), crate::instr::CodecError> {
        instr.encode(&mut self.method.code)
    }

    /// Grants entry capabilities to the method environment.
    // CEP:WHAT Adds capabilities to the method's entry environment.
    // CEP:WHY The verifier checks instruction requirements against this set.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1) amortized push.
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn grant(&mut self, cap: CapReq) {
        if !self.method.env_caps.contains(&cap) {
            self.method.env_caps.push(cap);
        }
    }

    /// Returns the current code byte length.
    // CEP:WHAT Reports the current encoded code length.
    // CEP:WHY Assemblers compute label offsets from emitted sizes.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn code_len(&self) -> usize {
        self.method.code.len()
    }
}

/// A complete UGB program image.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    /// Methods indexed by callee index.
    pub methods: Vec<Method>,
    /// Constant pool shared by all methods.
    pub constants: Vec<Constant>,
    /// FFI descriptor table.
    pub ffi_table: Vec<FfiDescriptor>,
    /// Supported language context identifiers.
    pub languages: Vec<u8>,
    /// Verifier bounds for this image.
    pub bounds: Bounds,
}

impl Program {
    /// The method at a callee index, if present.
    // CEP:WHAT Resolves a callee index to a method.
    // CEP:WHY Calls are by index; the interpreter needs O(1) lookup.
    // CEP:STATUS complete
    // CEP:FAILURE None when the index is out of range.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn method(&self, callee: u32) -> Option<&Method> {
        self.methods.get(callee as usize)
    }

    /// The constant at a pool index, if present.
    // CEP:WHAT Resolves a constant-pool index.
    // CEP:WHY LOAD_CONST resolves immediates through the pool.
    // CEP:STATUS complete
    // CEP:FAILURE None when the index is out of range.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn constant(&self, index: u32) -> Option<&Constant> {
        self.constants.get(index as usize)
    }
}
