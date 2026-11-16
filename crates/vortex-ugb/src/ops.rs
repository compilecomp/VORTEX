// CEP:FILE crates/vortex-ugb/src/ops.rs
// CEP:WHAT The UGB opcode space: every operation, its operand signature, and
//        its static capability requirements (UGB design 6.3, 6.4).
// CEP:WHY Encoding and verification are signature-driven; one static table
//        keeps the codec, the verifier, and the assembler in agreement.
// CEP:CLASS CEP-1
// CEP:STATUS complete
// CEP:FAILURE Unknown opcode bytes reject through TryFrom; the table never
//        panics and never invents entries.
// CEP:ASSUMES The 256-entry opcode space allocation documented below.
// CEP:COST O(1) table lookup per opcode via a match returning static data.
// CEP:EVIDENCE cargo test -p vortex-ugb
//! UGB opcode space.

/// Operand kinds understood by the signature-driven codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperandKind {
    /// Register index (u16).
    Reg,
    /// 32-bit signed immediate.
    Imm32,
    /// 64-bit signed immediate.
    Imm64,
    /// 64-bit IEEE-754 immediate.
    F64,
    /// Capability identifier (u16).
    CapId,
    /// Byte-offset label into the method code (i32).
    Label,
    /// Type tag (u8).
    TypeTag,
    /// FFI descriptor identifier (u32).
    FfiDesc,
    /// Comparison kind selector (u8).
    CmpKind,
    /// Argument or register count (u16).
    Count,
    /// Local variable slot (u16).
    Local,
    /// Constant-pool index (u32).
    ConstIdx,
    /// Language identifier (u8).
    LangId,
}

/// Static capability requirements (UGB design 6.4 capability types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CapReq {
    /// Read from GC-managed or capability-granted memory.
    MemRead,
    /// Write to GC-managed or capability-granted memory.
    MemWrite,
    /// Allocate heap objects.
    MemAlloc,
    /// Free manually-managed objects.
    MemFree,
    /// Call foreign functions.
    Ffi,
    /// Manage or transfer capabilities.
    CapAdmin,
    /// Inspect type information.
    TypeInspect,
    /// Modify type information.
    TypeModify,
    /// Read from I/O resources.
    IoRead,
    /// Write to I/O resources.
    IoWrite,
    /// Open network connections.
    IoNetwork,
    /// Access the filesystem.
    IoFile,
    /// Perform dynamic operations (dyn call).
    Dynamic,
    /// Evaluate code at runtime.
    Eval,
    /// Reflect over objects.
    Reflect,
    /// Switch language contexts.
    Polyglot,
    /// Call into another function's frame.
    Call,
}

/// Comparison kind selector for CMP and CMP_BRANCH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpKind {
    /// Equality.
    Eq,
    /// Inequality.
    Ne,
    /// Signed less than.
    Lt,
    /// Signed less or equal.
    Le,
    /// Signed greater than.
    Gt,
    /// Signed greater or equal.
    Ge,
}

impl CmpKind {
    /// Maps a selector byte to a comparison kind.
    // CEP:WHAT Decodes the CMP selector byte.
    // CEP:WHY CMP_BRANCH super bytecodes carry the kind inline.
    // CEP:STATUS complete
    // CEP:FAILURE None outside 0..6; the codec rejects other bytes first.
    // CEP:ASSUMES Selector byte was produced by the encoder.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    #[inline]
    pub fn from_byte(b: u8) -> Option<CmpKind> {
        match b {
            0 => Some(CmpKind::Eq),
            1 => Some(CmpKind::Ne),
            2 => Some(CmpKind::Lt),
            3 => Some(CmpKind::Le),
            4 => Some(CmpKind::Gt),
            5 => Some(CmpKind::Ge),
            _ => None,
        }
    }

    /// Encodes the comparison kind to its selector byte.
    // CEP:WHAT Encodes the comparison kind to a selector byte.
    // CEP:WHY Stable wire form for CMP and CMP_BRANCH.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; the mapping is total.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    #[inline]
    pub fn to_byte(self) -> u8 {
        match self {
            CmpKind::Eq => 0,
            CmpKind::Ne => 1,
            CmpKind::Lt => 2,
            CmpKind::Le => 3,
            CmpKind::Gt => 4,
            CmpKind::Ge => 5,
        }
    }
}

/// Every UGB operation with its stable opcode byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Op {
    /// No operation.
    Nop = 0x00,
    /// Load a constant-pool value into a register.
    LoadConst = 0x01,
    /// Load a local slot into a register.
    LoadLocal = 0x02,
    /// Store a register into a local slot.
    StoreLocal = 0x03,
    /// Copy register to register.
    Mov = 0x04,
    /// Integer addition.
    Add = 0x10,
    /// Integer subtraction.
    Sub = 0x11,
    /// Integer multiplication.
    Mul = 0x12,
    /// Integer division.
    Div = 0x13,
    /// Integer remainder.
    Rem = 0x14,
    /// Arithmetic negation.
    Neg = 0x15,
    /// Bitwise and.
    And = 0x20,
    /// Bitwise or.
    Or = 0x21,
    /// Bitwise exclusive or.
    Xor = 0x22,
    /// Logical shift left.
    Shl = 0x23,
    /// Logical shift right.
    Shr = 0x24,
    /// Bitwise complement.
    Not = 0x25,
    /// Compare two registers into the flag register.
    Cmp = 0x26,
    /// Unconditional jump.
    Jump = 0x30,
    /// Jump when the flag register holds true.
    JumpIfTrue = 0x31,
    /// Jump when the flag register holds false.
    JumpIfFalse = 0x32,
    /// Indirect call through a register.
    Call = 0x33,
    /// Method-table call by callee index.
    CallMethod = 0x34,
    /// Direct call to a verified target (inline-cache specialization).
    CallDirect = 0x35,
    /// Return from the current frame.
    Return = 0x36,
    /// Load from capability-granted memory.
    Load = 0x40,
    /// Store to capability-granted memory.
    Store = 0x41,
    /// Allocate a heap object.
    Alloc = 0x42,
    /// Free a manually-managed object.
    Free = 0x43,
    /// Dynamic type check.
    TypeCheck = 0x50,
    /// Explicit type cast.
    TypeCast = 0x51,
    /// Read the type tag of a value.
    TypeTag = 0x52,
    /// Call a foreign function.
    FfiCall = 0x60,
    /// Load from foreign memory.
    FfiLoad = 0x61,
    /// Store to foreign memory.
    FfiStore = 0x62,
    /// Assert a capability is available.
    CapCheck = 0x70,
    /// Grant a capability into the current environment.
    CapGrant = 0x71,
    /// Revoke a capability from the current environment.
    CapRevoke = 0x72,
    /// Switch the language context.
    LangSwitch = 0x80,
    /// Query the current language context.
    LangContext = 0x81,
    /// Call into a different language context.
    InteropCall = 0x82,
    /// Evaluate code held in a register.
    Eval = 0x90,
    /// Reflect over an object.
    Reflect = 0x91,
    /// Dynamic call by name.
    DynCall = 0x92,
    /// T0 specialization: guarded integer add (type assumption).
    AddInt = 0xA0,
    /// T0 specialization: immediate add.
    AddImm = 0xA1,
    /// T0 specialization: immediate subtract.
    SubImm = 0xA2,
    /// T0 super bytecode: load local, add immediate, store local.
    SuperLoadAddStore = 0xC0,
    /// T0 super bytecode: load local, call, store local.
    SuperLoadCallStore = 0xC1,
    /// T0 super bytecode: compare and conditional branch.
    SuperCmpBranch = 0xC2,
}

/// Static description of one opcode.
#[derive(Debug, Clone, Copy)]
pub struct OpInfo {
    /// Mnemonic used by the assembler and disassembler.
    pub name: &'static str,
    /// Operand signature in encoding order.
    pub sig: &'static [OperandKind],
    /// Capabilities the instruction requires at execution time.
    pub caps: &'static [CapReq],
}

const NO_OPERANDS: &[OperandKind] = &[];
const NO_CAPS: &[CapReq] = &[];

impl Op {
    /// Decodes an opcode byte.
    // CEP:WHAT Decodes a byte into the Op enum.
    // CEP:WHY The codec and verifier must reject unknown bytes deterministically.
    // CEP:STATUS complete
    // CEP:FAILURE None for unassigned bytes.
    // CEP:ASSUMES None.
    // CEP:COST O(1) match.
    // CEP:EVIDENCE cargo test -p vortex-ugb
    #[inline]
    pub fn from_byte(b: u8) -> Option<Op> {
        match b {
            0x00 => Some(Op::Nop),
            0x01 => Some(Op::LoadConst),
            0x02 => Some(Op::LoadLocal),
            0x03 => Some(Op::StoreLocal),
            0x04 => Some(Op::Mov),
            0x10 => Some(Op::Add),
            0x11 => Some(Op::Sub),
            0x12 => Some(Op::Mul),
            0x13 => Some(Op::Div),
            0x14 => Some(Op::Rem),
            0x15 => Some(Op::Neg),
            0x20 => Some(Op::And),
            0x21 => Some(Op::Or),
            0x22 => Some(Op::Xor),
            0x23 => Some(Op::Shl),
            0x24 => Some(Op::Shr),
            0x25 => Some(Op::Not),
            0x26 => Some(Op::Cmp),
            0x30 => Some(Op::Jump),
            0x31 => Some(Op::JumpIfTrue),
            0x32 => Some(Op::JumpIfFalse),
            0x33 => Some(Op::Call),
            0x34 => Some(Op::CallMethod),
            0x35 => Some(Op::CallDirect),
            0x36 => Some(Op::Return),
            0x40 => Some(Op::Load),
            0x41 => Some(Op::Store),
            0x42 => Some(Op::Alloc),
            0x43 => Some(Op::Free),
            0x50 => Some(Op::TypeCheck),
            0x51 => Some(Op::TypeCast),
            0x52 => Some(Op::TypeTag),
            0x60 => Some(Op::FfiCall),
            0x61 => Some(Op::FfiLoad),
            0x62 => Some(Op::FfiStore),
            0x70 => Some(Op::CapCheck),
            0x71 => Some(Op::CapGrant),
            0x72 => Some(Op::CapRevoke),
            0x80 => Some(Op::LangSwitch),
            0x81 => Some(Op::LangContext),
            0x82 => Some(Op::InteropCall),
            0x90 => Some(Op::Eval),
            0x91 => Some(Op::Reflect),
            0x92 => Some(Op::DynCall),
            0xA0 => Some(Op::AddInt),
            0xA1 => Some(Op::AddImm),
            0xA2 => Some(Op::SubImm),
            0xC0 => Some(Op::SuperLoadAddStore),
            0xC1 => Some(Op::SuperLoadCallStore),
            0xC2 => Some(Op::SuperCmpBranch),
            _ => None,
        }
    }

    /// The static signature and capability data for this opcode.
    // CEP:WHAT Returns the static OpInfo for this opcode.
    // CEP:WHY One table drives codec, verifier, and assembler together.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail; the mapping is total over the enum.
    // CEP:ASSUMES None.
    // CEP:COST O(1) match returning references to static data.
    // CEP:EVIDENCE cargo test -p vortex-ugb
    #[inline]
    pub fn info(self) -> OpInfo {
        use OperandKind as K;
        match self {
            Op::Nop => OpInfo {
                name: "NOP",
                sig: NO_OPERANDS,
                caps: NO_CAPS,
            },
            Op::LoadConst => OpInfo {
                name: "LOAD_CONST",
                sig: &[K::Reg, K::ConstIdx],
                caps: NO_CAPS,
            },
            Op::LoadLocal => OpInfo {
                name: "LOAD_LOCAL",
                sig: &[K::Reg, K::Local],
                caps: NO_CAPS,
            },
            Op::StoreLocal => OpInfo {
                name: "STORE_LOCAL",
                sig: &[K::Local, K::Reg],
                caps: NO_CAPS,
            },
            Op::Mov => OpInfo {
                name: "MOV",
                sig: &[K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Add => OpInfo {
                name: "ADD",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Sub => OpInfo {
                name: "SUB",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Mul => OpInfo {
                name: "MUL",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Div => OpInfo {
                name: "DIV",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Rem => OpInfo {
                name: "REM",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Neg => OpInfo {
                name: "NEG",
                sig: &[K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::And => OpInfo {
                name: "AND",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Or => OpInfo {
                name: "OR",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Xor => OpInfo {
                name: "XOR",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Shl => OpInfo {
                name: "SHL",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Shr => OpInfo {
                name: "SHR",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Not => OpInfo {
                name: "NOT",
                sig: &[K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::Cmp => OpInfo {
                name: "CMP",
                sig: &[K::Reg, K::Reg, K::Reg, K::CmpKind],
                caps: NO_CAPS,
            },
            Op::Jump => OpInfo {
                name: "JUMP",
                sig: &[K::Label],
                caps: NO_CAPS,
            },
            Op::JumpIfTrue => OpInfo {
                name: "JUMP_IF_TRUE",
                sig: &[K::Reg, K::Label],
                caps: NO_CAPS,
            },
            Op::JumpIfFalse => OpInfo {
                name: "JUMP_IF_FALSE",
                sig: &[K::Reg, K::Label],
                caps: NO_CAPS,
            },
            Op::Call => OpInfo {
                name: "CALL",
                sig: &[K::Reg, K::Count],
                caps: &[CapReq::Call],
            },
            Op::CallMethod => OpInfo {
                name: "CALL_METHOD",
                sig: &[K::Reg, K::ConstIdx, K::Count],
                caps: &[CapReq::Call],
            },
            Op::CallDirect => OpInfo {
                name: "CALL_DIRECT",
                sig: &[K::Reg, K::ConstIdx, K::Count],
                caps: &[CapReq::Call],
            },
            Op::Return => OpInfo {
                name: "RETURN",
                sig: &[K::Reg],
                caps: NO_CAPS,
            },
            Op::Load => OpInfo {
                name: "LOAD",
                sig: &[K::Reg, K::Reg, K::Count],
                caps: &[CapReq::MemRead],
            },
            Op::Store => OpInfo {
                name: "STORE",
                sig: &[K::Reg, K::Reg, K::Count],
                caps: &[CapReq::MemWrite],
            },
            Op::Alloc => OpInfo {
                name: "ALLOC",
                sig: &[K::Reg, K::Count],
                caps: &[CapReq::MemAlloc],
            },
            Op::Free => OpInfo {
                name: "FREE",
                sig: &[K::Reg],
                caps: &[CapReq::MemFree],
            },
            Op::TypeCheck => OpInfo {
                name: "TYPE_CHECK",
                sig: &[K::Reg, K::TypeTag],
                caps: &[CapReq::TypeInspect],
            },
            Op::TypeCast => OpInfo {
                name: "TYPE_CAST",
                sig: &[K::Reg, K::Reg, K::TypeTag],
                caps: NO_CAPS,
            },
            Op::TypeTag => OpInfo {
                name: "TYPE_TAG",
                sig: &[K::Reg, K::Reg],
                caps: &[CapReq::TypeInspect],
            },
            Op::FfiCall => OpInfo {
                name: "FFI_CALL",
                sig: &[K::Reg, K::FfiDesc, K::Count],
                caps: &[CapReq::Ffi],
            },
            Op::FfiLoad => OpInfo {
                name: "FFI_LOAD",
                sig: &[K::Reg, K::Reg, K::Count],
                caps: &[CapReq::Ffi],
            },
            Op::FfiStore => OpInfo {
                name: "FFI_STORE",
                sig: &[K::Reg, K::Reg, K::Count],
                caps: &[CapReq::Ffi],
            },
            Op::CapCheck => OpInfo {
                name: "CAP_CHECK",
                sig: &[K::CapId],
                caps: NO_CAPS,
            },
            Op::CapGrant => OpInfo {
                name: "CAP_GRANT",
                sig: &[K::CapId],
                caps: &[CapReq::CapAdmin],
            },
            Op::CapRevoke => OpInfo {
                name: "CAP_REVOKE",
                sig: &[K::CapId],
                caps: &[CapReq::CapAdmin],
            },
            Op::LangSwitch => OpInfo {
                name: "LANG_SWITCH",
                sig: &[K::LangId, K::Reg],
                caps: &[CapReq::Polyglot],
            },
            Op::LangContext => OpInfo {
                name: "LANG_CONTEXT",
                sig: &[K::Reg],
                caps: NO_CAPS,
            },
            Op::InteropCall => OpInfo {
                name: "INTEROP_CALL",
                sig: &[K::Reg, K::Reg, K::Count],
                caps: &[CapReq::Polyglot],
            },
            Op::Eval => OpInfo {
                name: "EVAL",
                sig: &[K::Reg],
                caps: &[CapReq::Eval],
            },
            Op::Reflect => OpInfo {
                name: "REFLECT",
                sig: &[K::Reg, K::Reg],
                caps: &[CapReq::Reflect],
            },
            Op::DynCall => OpInfo {
                name: "DYN_CALL",
                sig: &[K::Reg, K::Reg, K::Count],
                caps: &[CapReq::Dynamic],
            },
            Op::AddInt => OpInfo {
                name: "ADD_INT",
                sig: &[K::Reg, K::Reg, K::Reg],
                caps: NO_CAPS,
            },
            Op::AddImm => OpInfo {
                name: "ADD_IMM",
                sig: &[K::Reg, K::Reg, K::Imm32],
                caps: NO_CAPS,
            },
            Op::SubImm => OpInfo {
                name: "SUB_IMM",
                sig: &[K::Reg, K::Reg, K::Imm32],
                caps: NO_CAPS,
            },
            Op::SuperLoadAddStore => OpInfo {
                name: "LOAD_ADD_STORE",
                sig: &[K::Local, K::Imm32],
                caps: NO_CAPS,
            },
            Op::SuperLoadCallStore => OpInfo {
                name: "LOAD_CALL_STORE",
                sig: &[K::Local, K::ConstIdx, K::Count],
                caps: &[CapReq::Call],
            },
            Op::SuperCmpBranch => OpInfo {
                name: "CMP_BRANCH",
                sig: &[K::Reg, K::Reg, K::Label, K::CmpKind],
                caps: NO_CAPS,
            },
        }
    }

    /// Lists every defined opcode in wire order.
    // CEP:WHAT Enumerates all defined opcodes.
    // CEP:WHY The verifier self-test and documentation need the full set.
    // CEP:STATUS complete
    // CEP:FAILURE Cannot fail.
    // CEP:ASSUMES None.
    // CEP:COST O(opcodes) allocation of the returned array.
    // CEP:EVIDENCE cargo test -p vortex-ugb
    pub fn all() -> [Op; 50] {
        [
            Op::Nop,
            Op::LoadConst,
            Op::LoadLocal,
            Op::StoreLocal,
            Op::Mov,
            Op::Add,
            Op::Sub,
            Op::Mul,
            Op::Div,
            Op::Rem,
            Op::Neg,
            Op::And,
            Op::Or,
            Op::Xor,
            Op::Shl,
            Op::Shr,
            Op::Not,
            Op::Cmp,
            Op::Jump,
            Op::JumpIfTrue,
            Op::JumpIfFalse,
            Op::Call,
            Op::CallMethod,
            Op::CallDirect,
            Op::Return,
            Op::Load,
            Op::Store,
            Op::Alloc,
            Op::Free,
            Op::TypeCheck,
            Op::TypeCast,
            Op::TypeTag,
            Op::FfiCall,
            Op::FfiLoad,
            Op::FfiStore,
            Op::CapCheck,
            Op::CapGrant,
            Op::CapRevoke,
            Op::LangSwitch,
            Op::LangContext,
            Op::InteropCall,
            Op::Eval,
            Op::Reflect,
            Op::DynCall,
            Op::AddInt,
            Op::AddImm,
            Op::SubImm,
            Op::SuperLoadAddStore,
            Op::SuperLoadCallStore,
            Op::SuperCmpBranch,
        ]
    }
}

impl TryFrom<u8> for Op {
    type Error = ();

    // CEP:WHAT TryFrom maps a byte to an Op.
    // CEP:WHY Trait-based decode integrates with iterator pipelines.
    // CEP:STATUS complete
    // CEP:FAILURE Unit error for unassigned bytes.
    // CEP:ASSUMES None.
    // CEP:COST O(1).
    // CEP:EVIDENCE cargo test -p vortex-ugb
    #[inline]
    fn try_from(b: u8) -> Result<Op, ()> {
        Op::from_byte(b).ok_or(())
    }
}
