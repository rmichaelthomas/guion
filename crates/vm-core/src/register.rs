//! Register form of a verified function, built at load time.
//!
//! The stack bytecode stays the program of record: spans, jump targets, frames and the
//! native tier all keep using it. This form reads `Load` and `Const` operands where they
//! already live instead of copying them onto the stack, and folds a following `Store` or
//! conditional jump into the instruction that produced the value.
//!
//! Each instruction stands for a contiguous range of original instructions starting at
//! `start`. Running it charges exactly the fuel and allocation that range would. When
//! anything is out of the ordinary (a type the fast path does not handle, an error, a
//! charge that does not fit, an operation with no register form) it changes nothing,
//! rebuilds the stack the original code would have at `start`, and hands back to the stack
//! interpreter, which then produces the exact behavior and error.

use super::*;
use std::sync::OnceLock;

const NO_ENTRY: u32 = u32::MAX;
const NO_RECIPE: u32 = 0;

/// Where an operand lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    Local(u32),
    Const(u32),
    /// Distance from the top of the real stack.
    Stack(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dest {
    Push,
    Local(u32),
    /// Jump when the produced `Bool` equals `jump_when`.
    Branch {
        target: u32,
        jump_when: bool,
        backward: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Hand back to the stack interpreter at `start`.
    Exit,
    /// Copy operands still held in locals or constants onto the real stack.
    Materialize,
    Move,
    Pop,
    Jump {
        target: u32,
        backward: bool,
    },
    Not,
    /// The `Bool` operand of a conditional jump.
    Truth,
    IsOk,
    IsSome,
    Add,
    Subtract,
    Multiply,
    IntegerDivide,
    FloorDivide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Index,
    MapHas,
    /// `map_set` of an existing key of a unique map, assigned back to its own local.
    MapSetLocal,
    Length,
    Unwrap,
    GetField {
        record: u32,
        field: u32,
    },
    /// `Call` of a function that is exactly `Load 0; MakeSome; Return`.
    CallSome,
    /// `Call` of a function that is exactly `MakeNone; Return`.
    CallNone,
    MakeOk,
    MakeErr,
    MakeSome,
    MakeRecord {
        record: u32,
        count: u32,
    },
    /// `JsonTryGet` of a key that is present.
    JsonGet,
    /// `JsonTryGet; Unwrap; JsonTryInt` of a key that is present and an `i64`.
    JsonGetInt,
    /// `JsonTryGet; Unwrap; JsonTryText` of a key that is present and a string.
    JsonGetText,
    /// `Unwrap; JsonTryInt` of an `Ok` holding an `i64`.
    UnwrapJsonInt,
    /// `Unwrap; JsonTryText` of an `Ok` holding a JSON string.
    UnwrapJsonText,
    /// `TextParseInt` of a short run of digits.
    ParseInt,
    /// `Index; TextParseInt` of a short run of digits.
    IndexParseInt,
    /// `Unwrap; GetField` of an `Ok` or `Some` holding the record.
    UnwrapGetField {
        record: u32,
        field: u32,
    },
    /// A host operation at `op_ip`. Its pending operands go on the stack first.
    Host {
        op_ip: u32,
        check_result: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Instr {
    kind: Kind,
    a: Src,
    b: Src,
    c: Src,
    dest: Dest,
    /// The first original instruction this one stands for.
    start: u32,
    fuel: u32,
    /// How many operands come off the real stack.
    pops: u32,
    /// The `Load`s and `Const`s in the range, charged as the plain instructions would.
    charges_start: u32,
    charges_end: u32,
    /// How to rebuild the stack the original code has at `start`.
    recipe: u32,
}

/// One function in register form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterProgram {
    code: Vec<Instr>,
    /// Where to enter for each original instruction position.
    entries: Vec<u32>,
    consts: Vec<Value>,
    charges: Vec<Src>,
    recipes: Vec<Vec<Option<Src>>>,
    required_integer_digits: usize,
    required_text_bytes: usize,
}

/// Build the register form of every function of a verified artifact.
pub fn build_register_programs(artifact: &Artifact) -> RegisterPrograms {
    RegisterPrograms {
        constructors: artifact
            .functions
            .iter()
            .map(|function| match forwarded_host_operation(function) {
                Some(Op::MakeSome) => Some(Kind::CallSome),
                Some(Op::MakeNone) => Some(Kind::CallNone),
                _ => None,
            })
            .collect(),
        programs: artifact.functions.iter().map(|_| OnceLock::new()).collect(),
    }
}

/// One register form per function, translated the first time the function runs: most functions
/// of a large artifact, such as the embedded compiler, never run in a given process.
#[derive(Debug)]
pub struct RegisterPrograms {
    constructors: Vec<Option<Kind>>,
    programs: Vec<OnceLock<RegisterProgram>>,
}

impl RegisterPrograms {
    pub(super) fn get(
        &self,
        index: usize,
        function: &guion_model::Function,
    ) -> Option<&RegisterProgram> {
        self.programs.get(index).map(|program| {
            program.get_or_init(|| Translator::new(function, &self.constructors).translate())
        })
    }
}

impl RegisterProgram {
    /// Constants skip their boundary check here, and small integer results their digit
    /// check, so this form only runs under limits that could not refuse them.
    pub(super) fn usable_under(&self, limits: &VmLimits) -> bool {
        limits.max_integer_digits >= self.required_integer_digits
            && limits.max_text_bytes >= self.required_text_bytes
    }

    #[inline]
    pub(super) fn entry(&self, ip: usize) -> Option<u32> {
        match self.entries.get(ip) {
            Some(&pc) if pc != NO_ENTRY => Some(pc),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(super) fn covers_instruction_ranges(&self) -> bool {
        self.code.iter().any(|instruction| instruction.fuel > 1)
    }

    #[cfg(test)]
    pub(super) fn has_kind(&self, name: &str) -> bool {
        self.code
            .iter()
            .any(|instruction| format!("{:?}", instruction.kind).starts_with(name))
    }

    #[cold]
    fn materialize(&self, recipe: u32, locals: &[Value], stack: &mut Vec<Value>) {
        let entries = &self.recipes[recipe as usize];
        let real = entries.iter().filter(|entry| entry.is_none()).count();
        let base = stack.len() - real;
        for (offset, entry) in entries.iter().enumerate() {
            if let Some(source) = entry {
                let value = match *source {
                    Src::Local(slot) => locals[slot as usize].clone(),
                    Src::Const(index) => self.consts[index as usize].clone(),
                    Src::Stack(_) => unreachable!("recipes only hold locals and constants"),
                };
                stack.insert(base + offset, value);
            }
        }
    }
}

/// Host operations that never touch the instruction budget, so a local fuel reservation can
/// stay in place across them. Everything else goes through the stack interpreter, which
/// returns its reservation first.
fn host_operation_spends_no_fuel(op: &Op) -> bool {
    matches!(
        op,
        Op::JsonTryGet
            | Op::JsonTryIndex
            | Op::JsonTryGetText
            | Op::JsonTryGetInt
            | Op::JsonTryText
            | Op::JsonTryInt
            | Op::JsonTryBool
            | Op::JsonTryFloat
            | Op::JsonParse
            | Op::JsonInt
            | Op::JsonText
            | Op::JsonSetLocal(_)
            | Op::JsonPushLocal(_)
            | Op::FileNextLine
            | Op::CsvNext
            | Op::TextParseInt
            | Op::TextParseFloat
            | Op::TextCollapseWhitespace
            | Op::TimeParseTimestamp
            | Op::TimeFormatUtc
            | Op::TimeFormatUtcDate
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Virtual {
    Deferred(Src),
    Real,
}

struct Translator<'a> {
    function: &'a guion_model::Function,
    constructors: &'a [Option<Kind>],
    is_leader: Vec<bool>,
    code: Vec<Instr>,
    consts: Vec<Value>,
    charges: Vec<Src>,
    recipes: Vec<Vec<Option<Src>>>,
    /// The stack above the start of the current block. Anything below it is real.
    stack: Vec<Virtual>,
    /// The top `pending` entries are `Load`s and `Const`s no instruction has charged yet.
    pending: usize,
    pending_start: usize,
    required_integer_digits: usize,
    required_text_bytes: usize,
}

impl<'a> Translator<'a> {
    fn new(function: &'a guion_model::Function, constructors: &'a [Option<Kind>]) -> Self {
        let mut is_leader = vec![false; function.code.len() + 1];
        for instruction in &function.code {
            if let Op::Jump(target) | Op::JumpIfFalse(target) = &instruction.op {
                if let Some(leader) = is_leader.get_mut(*target) {
                    *leader = true;
                }
            }
        }
        Self {
            function,
            constructors,
            is_leader,
            code: Vec::new(),
            consts: Vec::new(),
            charges: Vec::new(),
            recipes: vec![Vec::new()],
            stack: Vec::new(),
            pending: 0,
            pending_start: 0,
            required_integer_digits: 19,
            required_text_bytes: 0,
        }
    }

    fn op_at(&self, ip: usize) -> Option<&'a Op> {
        self.function
            .code
            .get(ip)
            .map(|instruction| &instruction.op)
    }

    /// The next instruction, if control can only reach it from here.
    fn follower(&self, ip: usize) -> Option<&'a Op> {
        if self.is_leader.get(ip).copied().unwrap_or(true) {
            None
        } else {
            self.op_at(ip)
        }
    }

    fn deferred_anywhere(&self) -> bool {
        self.stack
            .iter()
            .any(|entry| matches!(entry, Virtual::Deferred(_)))
    }

    fn holds_local(&self, slot: u32) -> bool {
        self.stack.contains(&Virtual::Deferred(Src::Local(slot)))
    }

    /// The recipe for the stack below the pending entries.
    fn recipe(&mut self) -> u32 {
        let settled = &self.stack[..self.stack.len() - self.pending];
        if !settled
            .iter()
            .any(|entry| matches!(entry, Virtual::Deferred(_)))
        {
            return NO_RECIPE;
        }
        let entries = settled
            .iter()
            .map(|entry| match entry {
                Virtual::Deferred(source) => Some(*source),
                Virtual::Real => None,
            })
            .collect();
        self.recipes.push(entries);
        (self.recipes.len() - 1) as u32
    }

    fn start_at(&self, ip: usize) -> usize {
        if self.pending > 0 {
            self.pending_start
        } else {
            ip
        }
    }

    fn take_pending_charges(&mut self) -> (u32, u32) {
        let charges_start = self.charges.len() as u32;
        let first = self.stack.len() - self.pending;
        for entry in &self.stack[first..] {
            if let Virtual::Deferred(source) = entry {
                self.charges.push(*source);
            }
        }
        (charges_start, self.charges.len() as u32)
    }

    fn push_deferred(&mut self, source: Src, ip: usize) {
        if self.pending == 0 {
            self.pending_start = ip;
        }
        self.pending += 1;
        self.stack.push(Virtual::Deferred(source));
    }

    fn const_index(&mut self, value: &Value) -> Option<u32> {
        match value {
            Value::Int(int) => {
                self.required_integer_digits = self.required_integer_digits.max(int.digit_count());
            }
            Value::Text(text) => {
                self.required_text_bytes = self.required_text_bytes.max(text.len());
            }
            Value::Bool(_) | Value::Float64(_) | Value::Unit | Value::OptionNone => {}
            _ => return None,
        }
        self.consts.push(value.clone());
        Some((self.consts.len() - 1) as u32)
    }

    /// Emit an instruction taking `arguments` operands. `folded` more original instructions
    /// are covered by `dest`.
    fn emit(&mut self, kind: Kind, arguments: usize, ip: usize, dest: Dest, folded: u32) {
        let recipe = self.recipe();
        let start = self.start_at(ip) as u32;
        let (charges_start, charges_end) = self.take_pending_charges();
        let fuel = self.pending as u32 + 1 + folded;
        self.pending = 0;
        let mut operands = [Src::Stack(0); 3];
        let mut pops = 0;
        for position in (0..arguments).rev() {
            operands[position] = match self.stack.pop() {
                Some(Virtual::Deferred(source)) => source,
                Some(Virtual::Real) | None => {
                    pops += 1;
                    Src::Stack(pops - 1)
                }
            };
        }
        if dest == Dest::Push {
            self.stack.push(Virtual::Real);
        }
        self.code.push(Instr {
            kind,
            a: operands[0],
            b: operands[1],
            c: operands[2],
            dest,
            start,
            fuel,
            pops,
            charges_start,
            charges_end,
            recipe,
        });
    }

    /// Emit an instruction whose value is used by what follows, folding that when possible.
    /// Returns how many original instructions after `ip` it covered.
    fn emit_value(&mut self, kind: Kind, arguments: usize, ip: usize) -> usize {
        self.emit_chain(kind, arguments, ip, 0)
    }

    /// As `emit_value`, for an instruction that also stands for the `chained` after `ip`.
    fn emit_chain(&mut self, kind: Kind, arguments: usize, ip: usize, chained: u32) -> usize {
        let remaining = self.stack.len().saturating_sub(arguments);
        let next = ip + chained as usize + 1;
        let store = match self.follower(next) {
            Some(Op::Store(slot))
                if !self.stack[..remaining]
                    .contains(&Virtual::Deferred(Src::Local(*slot as u32))) =>
            {
                Some(*slot as u32)
            }
            _ => None,
        };
        if let Some(slot) = store {
            self.emit(kind, arguments, ip, Dest::Local(slot), chained + 1);
            return chained as usize + 1;
        }
        self.emit(kind, arguments, ip, Dest::Push, chained);
        chained as usize
    }

    /// As `emit_value`, for a `Bool` result that a conditional jump may consume directly.
    fn emit_bool(&mut self, kind: Kind, arguments: usize, ip: usize) -> usize {
        let remaining = self.stack.len().saturating_sub(arguments);
        let settled = !self.stack[..remaining]
            .iter()
            .any(|entry| matches!(entry, Virtual::Deferred(_)));
        if settled {
            let branch = match (self.follower(ip + 1), self.follower(ip + 2)) {
                (Some(Op::JumpIfFalse(target)), _) => Some((*target, false, ip + 1, 1)),
                (Some(Op::Not), Some(Op::JumpIfFalse(target))) => Some((*target, true, ip + 2, 2)),
                _ => None,
            };
            if let Some((target, jump_when, jump_ip, folded)) = branch {
                let dest = Dest::Branch {
                    target: target as u32,
                    jump_when,
                    backward: target <= jump_ip,
                };
                self.emit(kind, arguments, ip, dest, folded as u32);
                return folded;
            }
        }
        self.emit_value(kind, arguments, ip)
    }

    /// Make the virtual stack real, so the current position is a clean boundary.
    fn realize_all(&mut self, ip: usize) {
        let pending = self.pending;
        let first_pending = self.stack.len() - pending;
        let start = self.start_at(ip);
        let recipe = self.recipe();
        if recipe != NO_RECIPE {
            self.code.push(Instr {
                kind: Kind::Materialize,
                a: Src::Stack(0),
                b: Src::Stack(0),
                c: Src::Stack(0),
                dest: Dest::Push,
                start: start as u32,
                fuel: 0,
                pops: 0,
                charges_start: 0,
                charges_end: 0,
                recipe,
            });
        }
        for offset in 0..pending {
            let Virtual::Deferred(source) = self.stack[first_pending + offset] else {
                unreachable!("pending entries are always deferred");
            };
            let charges_start = self.charges.len() as u32;
            self.charges.push(source);
            self.code.push(Instr {
                kind: Kind::Move,
                a: source,
                b: Src::Stack(0),
                c: Src::Stack(0),
                dest: Dest::Push,
                start: (start + offset) as u32,
                fuel: 1,
                pops: 0,
                charges_start,
                charges_end: charges_start + 1,
                recipe: NO_RECIPE,
            });
        }
        self.pending = 0;
        self.stack.clear();
    }

    /// Run a host operation here. It leaves an unknown number of values, all of them real.
    fn emit_host(&mut self, op: &Op, ip: usize) {
        let start = self.start_at(ip) as u32;
        let recipe = self.recipe();
        if recipe != NO_RECIPE {
            self.code.push(Instr {
                kind: Kind::Materialize,
                a: Src::Stack(0),
                b: Src::Stack(0),
                c: Src::Stack(0),
                dest: Dest::Push,
                start,
                fuel: 0,
                pops: 0,
                charges_start: 0,
                charges_end: 0,
                recipe,
            });
        }
        let (charges_start, charges_end) = self.take_pending_charges();
        let fuel = self.pending as u32 + 1;
        self.pending = 0;
        self.stack.clear();
        self.code.push(Instr {
            kind: Kind::Host {
                op_ip: ip as u32,
                check_result: host_result_needs_limit_check(op),
            },
            a: Src::Stack(0),
            b: Src::Stack(0),
            c: Src::Stack(0),
            dest: Dest::Push,
            start,
            fuel,
            pops: 0,
            charges_start,
            charges_end,
            recipe: NO_RECIPE,
        });
    }

    /// Leave this position to the stack interpreter.
    fn emit_exit(&mut self, ip: usize) {
        let recipe = self.recipe();
        let start = self.start_at(ip) as u32;
        self.pending = 0;
        self.stack.clear();
        self.code.push(Instr {
            kind: Kind::Exit,
            a: Src::Stack(0),
            b: Src::Stack(0),
            c: Src::Stack(0),
            dest: Dest::Push,
            start,
            fuel: 0,
            pops: 0,
            charges_start: 0,
            charges_end: 0,
            recipe,
        });
    }

    fn translate(mut self) -> RegisterProgram {
        let length = self.function.code.len();
        let mut ip = 0;
        while ip < length {
            if self.is_leader[ip] {
                self.realize_all(ip);
            }
            let covered = self.translate_one(ip);
            ip += 1 + covered;
        }
        self.realize_all(length);
        self.emit_exit(length);
        self.finish(length)
    }

    fn translate_one(&mut self, ip: usize) -> usize {
        let code = &self.function.code;
        match &code[ip].op {
            Op::Load(slot) => {
                self.push_deferred(Src::Local(*slot as u32), ip);
                0
            }
            Op::Const(value) => {
                match self.const_index(value) {
                    Some(index) => self.push_deferred(Src::Const(index), ip),
                    None => self.emit_exit(ip),
                }
                0
            }
            Op::Store(slot) => {
                let slot = *slot as u32;
                let top = self.stack.pop();
                let held_below = self.holds_local(slot);
                if let Some(top) = top {
                    self.stack.push(top);
                }
                if held_below {
                    self.realize_all(ip);
                }
                self.emit(Kind::Move, 1, ip, Dest::Local(slot), 0);
                0
            }
            Op::Pop => {
                self.emit(Kind::Pop, 1, ip, Dest::Local(u32::MAX), 0);
                0
            }
            Op::MakeNone => {
                let index = self.const_index(&Value::OptionNone);
                match index {
                    Some(index) => self.push_deferred(Src::Const(index), ip),
                    None => self.emit_exit(ip),
                }
                0
            }
            Op::MakeOk => self.emit_value(Kind::MakeOk, 1, ip),
            Op::MakeErr => self.emit_value(Kind::MakeErr, 1, ip),
            Op::MakeSome => self.emit_value(Kind::MakeSome, 1, ip),
            Op::MakeRecord(record, count) => {
                self.realize_all(ip);
                let kind = Kind::MakeRecord {
                    record: *record as u32,
                    count: *count as u32,
                };
                let covered = self.emit_value(kind, 0, ip);
                if let Some(last) = self.code.last_mut() {
                    last.pops = *count as u32;
                }
                covered
            }
            Op::Add => self.emit_value(Kind::Add, 2, ip),
            Op::Subtract => self.emit_value(Kind::Subtract, 2, ip),
            Op::Multiply => self.emit_value(Kind::Multiply, 2, ip),
            Op::IntegerDivide => self.emit_value(Kind::IntegerDivide, 2, ip),
            Op::FloorDivide => self.emit_value(Kind::FloorDivide, 2, ip),
            Op::Remainder => self.emit_value(Kind::Remainder, 2, ip),
            Op::Index => match self.follower(ip + 1) {
                Some(Op::TextParseInt) => self.emit_chain(Kind::IndexParseInt, 2, ip, 1),
                _ => self.emit_value(Kind::Index, 2, ip),
            },
            Op::TextParseInt => self.emit_value(Kind::ParseInt, 1, ip),
            Op::Length => self.emit_value(Kind::Length, 1, ip),
            Op::Unwrap => match self.follower(ip + 1) {
                Some(Op::JsonTryInt) => self.emit_chain(Kind::UnwrapJsonInt, 1, ip, 1),
                Some(Op::JsonTryText) => self.emit_chain(Kind::UnwrapJsonText, 1, ip, 1),
                Some(Op::GetField(record, field)) => self.emit_chain(
                    Kind::UnwrapGetField {
                        record: *record as u32,
                        field: *field as u32,
                    },
                    1,
                    ip,
                    1,
                ),
                _ => self.emit_value(Kind::Unwrap, 1, ip),
            },
            Op::JsonTryGet => match (self.follower(ip + 1), self.follower(ip + 2)) {
                (Some(Op::Unwrap), Some(Op::JsonTryInt)) => {
                    self.emit_chain(Kind::JsonGetInt, 2, ip, 2)
                }
                (Some(Op::Unwrap), Some(Op::JsonTryText)) => {
                    self.emit_chain(Kind::JsonGetText, 2, ip, 2)
                }
                _ => self.emit_value(Kind::JsonGet, 2, ip),
            },
            Op::GetField(record, field) => self.emit_value(
                Kind::GetField {
                    record: *record as u32,
                    field: *field as u32,
                },
                1,
                ip,
            ),
            Op::Not => self.emit_bool(Kind::Not, 1, ip),
            Op::IsOk => self.emit_bool(Kind::IsOk, 1, ip),
            Op::IsSome => self.emit_bool(Kind::IsSome, 1, ip),
            Op::MapHas => self.emit_bool(Kind::MapHas, 2, ip),
            Op::Equal => self.emit_bool(Kind::Equal, 2, ip),
            Op::NotEqual => self.emit_bool(Kind::NotEqual, 2, ip),
            Op::Less => self.emit_bool(Kind::Less, 2, ip),
            Op::LessEqual => self.emit_bool(Kind::LessEqual, 2, ip),
            Op::Greater => self.emit_bool(Kind::Greater, 2, ip),
            Op::GreaterEqual => self.emit_bool(Kind::GreaterEqual, 2, ip),
            Op::MapSet => {
                let store = match self.follower(ip + 1) {
                    Some(Op::Store(slot)) => *slot as u32,
                    _ => {
                        self.emit_exit(ip);
                        return 0;
                    }
                };
                let map_position = self.stack.len().checked_sub(3);
                // The map must not also be the key, the value, or waiting further down.
                let map_uses = self
                    .stack
                    .iter()
                    .filter(|entry| **entry == Virtual::Deferred(Src::Local(store)))
                    .count();
                let eligible = map_uses == 1
                    && map_position.is_some_and(|position| {
                        self.stack[position] == Virtual::Deferred(Src::Local(store))
                    });
                if !eligible {
                    self.emit_exit(ip);
                    return 0;
                }
                self.emit(Kind::MapSetLocal, 3, ip, Dest::Local(store), 1);
                1
            }
            Op::Jump(target) => {
                let target = *target;
                self.realize_all(ip);
                self.emit(
                    Kind::Jump {
                        target: target as u32,
                        backward: target <= ip,
                    },
                    0,
                    ip,
                    Dest::Local(u32::MAX),
                    0,
                );
                0
            }
            Op::JumpIfFalse(target) => {
                let target = *target;
                let top = self.stack.pop();
                let settled = !self.deferred_anywhere();
                if let Some(top) = top {
                    self.stack.push(top);
                }
                if !settled {
                    self.realize_all(ip);
                }
                self.emit(
                    Kind::Truth,
                    1,
                    ip,
                    Dest::Branch {
                        target: target as u32,
                        jump_when: false,
                        backward: target <= ip,
                    },
                    0,
                );
                0
            }
            Op::Call(target, _) => {
                match self.constructors.get(*target).copied().flatten() {
                    Some(Kind::CallSome) => {
                        self.realize_all(ip);
                        // The call, and the callee's `Load`, `MakeSome` and `Return`.
                        self.emit(Kind::CallSome, 1, ip, Dest::Push, 3);
                    }
                    Some(Kind::CallNone) => {
                        self.realize_all(ip);
                        // The call, and the callee's `MakeNone` and `Return`.
                        self.emit(Kind::CallNone, 0, ip, Dest::Push, 2);
                    }
                    _ => self.emit_exit(ip),
                }
                0
            }
            op if host_operation_spends_no_fuel(op) => {
                self.emit_host(op, ip);
                0
            }
            _ => {
                self.emit_exit(ip);
                0
            }
        }
    }

    fn finish(mut self, length: usize) -> RegisterProgram {
        let mut clean = vec![NO_ENTRY; length + 1];
        for (pc, instruction) in self.code.iter().enumerate() {
            let position = &mut clean[instruction.start as usize];
            if instruction.recipe == NO_RECIPE
                && instruction.kind != Kind::Materialize
                && *position == NO_ENTRY
            {
                *position = pc as u32;
            }
        }
        let resolve = |target: u32| clean.get(target as usize).copied().unwrap_or(NO_ENTRY);
        for instruction in &mut self.code {
            if let Kind::Jump { target, .. } = &mut instruction.kind {
                *target = resolve(*target);
            }
            if let Dest::Branch { target, .. } = &mut instruction.dest {
                *target = resolve(*target);
            }
        }
        // Entering only to leave at once would cost more than never entering.
        let entries: Vec<u32> = clean
            .iter()
            .map(|&pc| match self.code.get(pc as usize) {
                Some(instruction) if instruction.kind != Kind::Exit => pc,
                _ => NO_ENTRY,
            })
            .collect();
        RegisterProgram {
            code: self.code,
            entries,
            consts: self.consts,
            charges: self.charges,
            recipes: self.recipes,
            required_integer_digits: self.required_integer_digits,
            required_text_bytes: self.required_text_bytes,
        }
    }
}

pub(super) struct RegisterExit {
    pub ip: usize,
    pub poll_cancellation: bool,
}

pub(super) struct RegisterRun<'a, 'fuel, H: VmHost> {
    pub host: &'a mut H,
    pub artifact: &'a Artifact,
    pub function: &'a guion_model::Function,
    pub locals: &'a mut Vec<Value>,
    pub stack: &'a mut Vec<Value>,
    pub fuel: &'a mut InstructionFuel<'fuel>,
    pub budget: &'a Arc<Budget>,
    pub limits: &'a VmLimits,
    pub depth: usize,
}

/// Run from `pc` until an instruction hands back to the stack interpreter.
pub(super) fn run_registers<H: VmHost>(
    program: &RegisterProgram,
    mut pc: usize,
    run: RegisterRun<'_, '_, H>,
) -> Result<RegisterExit, RuntimeError> {
    let RegisterRun {
        host,
        artifact,
        function,
        locals,
        stack,
        fuel,
        budget,
        limits,
        depth,
    } = run;
    // The hash of the last map key. Python keeps a string's hash in the string; a program
    // tends to look one key up in several maps in a row, so this gets most of the same gain.
    let mut last_key: Option<(SharedText, u64)> = None;
    loop {
        let instruction = &program.code[pc];
        let mut key_hash = |key: &SharedText| match &last_key {
            Some((last, hash)) if SharedText::ptr_eq(last, key) => *hash,
            _ => {
                let hash = TextMap::hash_key(key);
                last_key = Some((SharedText::clone(key), hash));
                hash
            }
        };

        macro_rules! exit {
            () => {{
                if instruction.recipe != NO_RECIPE {
                    program.materialize(instruction.recipe, locals, stack);
                }
                return Ok(RegisterExit {
                    ip: instruction.start as usize,
                    poll_cancellation: false,
                });
            }};
        }
        macro_rules! read {
            ($source:expr) => {
                match $source {
                    Src::Local(slot) => &locals[slot as usize],
                    Src::Const(index) => &program.consts[index as usize],
                    Src::Stack(distance) => match stack.len().checked_sub(distance as usize + 1) {
                        Some(position) => &stack[position],
                        None => exit!(),
                    },
                }
            };
        }
        // Everything that can refuse, checked together: a refusal takes nothing, so the
        // stack interpreter replays the range and reports it at its own instruction.
        macro_rules! commit {
            ($own:expr) => {{
                let needed = u64::from(instruction.fuel);
                if fuel.remaining < needed {
                    exit!();
                }
                let mut charge: usize = $own;
                for source in &program.charges
                    [instruction.charges_start as usize..instruction.charges_end as usize]
                {
                    let value = match *source {
                        Src::Local(slot) => &locals[slot as usize],
                        Src::Const(index) => &program.consts[index as usize],
                        Src::Stack(_) => unreachable!("charges only name locals and constants"),
                    };
                    charge = charge.saturating_add(cloned_value_allocation_fast(value));
                }
                if charge != 0 && !budget.allocate(charge) {
                    exit!();
                }
                fuel.remaining -= needed;
            }};
        }
        macro_rules! drop_operands {
            () => {
                for _ in 0..instruction.pops {
                    if let Some(value) = stack.pop() {
                        drop_value_fast(value);
                    }
                }
            };
        }
        macro_rules! jump {
            ($target:expr, $backward:expr) => {{
                let target = $target as usize;
                if $backward && (budget.is_cancelled() || host.externally_cancelled()) {
                    return Ok(RegisterExit {
                        ip: program.code[target].start as usize,
                        poll_cancellation: true,
                    });
                }
                pc = target;
                continue;
            }};
        }
        macro_rules! finish_value {
            ($value:expr) => {{
                let value = $value;
                match instruction.dest {
                    Dest::Push => stack.push(value),
                    Dest::Local(slot) => {
                        drop_value_fast(std::mem::replace(&mut locals[slot as usize], value));
                    }
                    Dest::Branch { .. } => unreachable!("only Bool results branch"),
                }
                pc += 1;
                continue;
            }};
        }
        macro_rules! finish_bool {
            ($value:expr) => {{
                let value: bool = $value;
                drop_operands!();
                if let Dest::Branch {
                    target,
                    jump_when,
                    backward,
                } = instruction.dest
                {
                    if value == jump_when {
                        jump!(target, backward);
                    }
                    pc += 1;
                    continue;
                }
                finish_value!(Value::Bool(value))
            }};
        }
        macro_rules! compare {
            ($int:expr, $float:expr, $text:expr) => {{
                let result = match (read!(instruction.a), read!(instruction.b)) {
                    (Value::Int(left), Value::Int(right)) => match (left.small(), right.small()) {
                        (Some(left), Some(right)) => {
                            let compare: fn(&i64, &i64) -> bool = $int;
                            compare(&left, &right)
                        }
                        _ => exit!(),
                    },
                    (Value::Float64(left), Value::Float64(right)) => {
                        let compare: fn(&f64, &f64) -> bool = $float;
                        compare(&left.get(), &right.get())
                    }
                    (Value::Text(left), Value::Text(right)) => {
                        let compare: fn(&str, &str) -> bool = $text;
                        compare(left, right)
                    }
                    _ => exit!(),
                };
                commit!(0);
                finish_bool!(result)
            }};
        }
        macro_rules! arithmetic {
            ($int:expr, $float:expr) => {{
                let result = match (read!(instruction.a), read!(instruction.b)) {
                    (Value::Int(left), Value::Int(right)) => {
                        let (Some(left), Some(right)) = (left.small(), right.small()) else {
                            exit!()
                        };
                        let compute: fn(i64, i64) -> Option<i64> = $int;
                        match compute(left, right) {
                            Some(value) => Value::Int(BigInt::from_i64(value)),
                            None => exit!(),
                        }
                    }
                    (Value::Float64(left), Value::Float64(right)) => {
                        let compute: fn(f64, f64) -> f64 = $float;
                        match Float64::new(compute(left.get(), right.get())) {
                            Some(value) => Value::Float64(value),
                            None => exit!(),
                        }
                    }
                    _ => exit!(),
                };
                commit!(0);
                drop_operands!();
                finish_value!(result)
            }};
        }

        match instruction.kind {
            Kind::Exit => exit!(),
            Kind::Materialize => {
                program.materialize(instruction.recipe, locals, stack);
                pc += 1;
            }
            Kind::Move => {
                commit!(0);
                let value = match instruction.a {
                    Src::Stack(_) => match stack.pop() {
                        Some(value) => value,
                        None => unreachable!("verified bytecode has an operand to store"),
                    },
                    Src::Local(slot) => clone_value_fast(&locals[slot as usize]),
                    Src::Const(index) => clone_value_fast(&program.consts[index as usize]),
                };
                finish_value!(value)
            }
            Kind::Pop => {
                commit!(0);
                drop_operands!();
                pc += 1;
            }
            Kind::Jump { target, backward } => {
                commit!(0);
                jump!(target, backward);
            }
            Kind::Truth | Kind::Not => {
                let Value::Bool(value) = read!(instruction.a) else {
                    exit!()
                };
                let value = *value != (instruction.kind == Kind::Not);
                commit!(0);
                finish_bool!(value)
            }
            Kind::IsOk => {
                let value = match read!(instruction.a) {
                    Value::ResultOk(_) => true,
                    Value::ResultErr(_) => false,
                    _ => exit!(),
                };
                commit!(0);
                finish_bool!(value)
            }
            Kind::IsSome => {
                let value = match read!(instruction.a) {
                    Value::OptionSome(_) => true,
                    Value::OptionNone => false,
                    _ => exit!(),
                };
                commit!(0);
                finish_bool!(value)
            }
            Kind::Add => arithmetic!(i64::checked_add, |left, right| left + right),
            Kind::Subtract => arithmetic!(i64::checked_sub, |left, right| left - right),
            Kind::Multiply => arithmetic!(i64::checked_mul, |left, right| left * right),
            // A float operand gives a non-finite result here, which leaves, and the stack
            // interpreter reports the type error; so does a zero divisor.
            Kind::IntegerDivide => arithmetic!(i64::checked_div, |_, _| f64::NAN),
            Kind::FloorDivide => arithmetic!(
                |left, right| {
                    let (quotient, remainder) =
                        (left.checked_div(right)?, left.checked_rem(right)?);
                    let adjust = remainder != 0 && (left < 0) != (right < 0);
                    Some(if adjust { quotient - 1 } else { quotient })
                },
                |_, _| f64::NAN
            ),
            Kind::Remainder => arithmetic!(
                |left, right| {
                    let remainder = left.checked_rem(right)?;
                    let adjust = remainder != 0 && (left < 0) != (right < 0);
                    Some(if adjust { remainder + right } else { remainder })
                },
                |_, _| f64::NAN
            ),
            Kind::Equal | Kind::NotEqual => {
                let equal = read!(instruction.a) == read!(instruction.b);
                commit!(0);
                finish_bool!(equal == (instruction.kind == Kind::Equal))
            }
            Kind::Less => compare!(i64::lt, f64::lt, |left, right| left < right),
            Kind::LessEqual => compare!(i64::le, f64::le, |left, right| left <= right),
            Kind::Greater => compare!(i64::gt, f64::gt, |left, right| left > right),
            Kind::GreaterEqual => compare!(i64::ge, f64::ge, |left, right| left >= right),
            Kind::Index => {
                let item = match (read!(instruction.a), read!(instruction.b)) {
                    (Value::Map(items), Value::Text(key)) => {
                        items.get_hashed(key_hash(key), key.as_ref())
                    }
                    (Value::List(items), Value::Int(position)) => position
                        .small()
                        .and_then(|position| usize::try_from(position).ok())
                        .and_then(|position| items.get(position)),
                    _ => None,
                };
                let Some(item) = item else { exit!() };
                commit!(cloned_value_allocation_fast(item));
                let value = clone_value_fast(item);
                drop_operands!();
                finish_value!(value)
            }
            Kind::MapHas => {
                let (Value::Map(items), Value::Text(key)) =
                    (read!(instruction.a), read!(instruction.b))
                else {
                    exit!()
                };
                let value = items.get_hashed(key_hash(key), key.as_ref()).is_some();
                commit!(0);
                finish_bool!(value)
            }
            Kind::MapSetLocal => {
                let Src::Local(slot) = instruction.a else {
                    exit!()
                };
                if !matches!(locals[slot as usize], Value::Map(_)) {
                    exit!();
                }
                // Out of the local so the key and value can still be read from the others.
                let Value::Map(mut map) =
                    std::mem::replace(&mut locals[slot as usize], Value::Unit)
                else {
                    unreachable!("the local was checked just above")
                };
                macro_rules! leave {
                    () => {{
                        locals[slot as usize] = Value::Map(map);
                        exit!()
                    }};
                }
                let existing = {
                    let key = match read_operand(program, locals, stack, instruction.b) {
                        Some(Value::Text(key)) if key.len() <= limits.max_text_bytes => key,
                        _ => leave!(),
                    };
                    let hash = key_hash(key);
                    Arc::get_mut(&mut map)
                        .and_then(|entries| entries.get_mut_hashed(hash, key.as_ref()))
                };
                let Some(existing) = existing else { leave!() };
                let within_limits = match read_operand(program, locals, stack, instruction.c) {
                    Some(value) => value_limit_error(value, limits, Span::default()).is_none(),
                    None => false,
                };
                if !within_limits {
                    leave!();
                }
                if fuel.remaining < u64::from(instruction.fuel) {
                    leave!();
                }
                let mut charge = 0usize;
                for source in &program.charges
                    [instruction.charges_start as usize..instruction.charges_end as usize]
                {
                    let value = match *source {
                        // The map itself charges nothing, and it is out of its local here.
                        Src::Local(charged) if charged == slot => continue,
                        Src::Local(charged) => &locals[charged as usize],
                        Src::Const(index) => &program.consts[index as usize],
                        Src::Stack(_) => unreachable!("charges only name locals and constants"),
                    };
                    charge = charge.saturating_add(cloned_value_allocation_fast(value));
                }
                if charge != 0 && !budget.allocate(charge) {
                    leave!();
                }
                fuel.remaining -= u64::from(instruction.fuel);
                let value = match instruction.c {
                    Src::Stack(_) => match stack.pop() {
                        Some(value) => value,
                        None => unreachable!("the value was read from the stack just above"),
                    },
                    Src::Local(source) => clone_value_fast(&locals[source as usize]),
                    Src::Const(index) => clone_value_fast(&program.consts[index as usize]),
                };
                drop_value_fast(std::mem::replace(existing, value));
                // The value is already off the stack if it was there; the key may be next.
                let value_popped = u32::from(matches!(instruction.c, Src::Stack(_)));
                for _ in value_popped..instruction.pops {
                    drop(stack.pop());
                }
                locals[slot as usize] = Value::Map(map);
                pc += 1;
            }
            Kind::Length => {
                let length = match read!(instruction.a) {
                    Value::List(items) => items.len(),
                    Value::Float64List(items) => items.len(),
                    Value::Map(items) => items.len(),
                    Value::Bytes(items) => items.len(),
                    Value::Text(text) => text.chars().count(),
                    _ => exit!(),
                };
                commit!(0);
                drop_operands!();
                finish_value!(Value::Int(BigInt::from_i64(length as i64)))
            }
            Kind::Unwrap => {
                if !matches!(
                    read!(instruction.a),
                    Value::ResultOk(_) | Value::OptionSome(_)
                ) {
                    exit!();
                }
                commit!(0);
                let value = match instruction.a {
                    Src::Stack(_) => match stack.pop() {
                        Some(Value::ResultOk(payload) | Value::OptionSome(payload)) => *payload,
                        _ => unreachable!("the operand was checked just above"),
                    },
                    source => match read!(source) {
                        Value::ResultOk(payload) | Value::OptionSome(payload) => {
                            clone_value_fast(payload)
                        }
                        _ => unreachable!("the operand was checked just above"),
                    },
                };
                finish_value!(value)
            }
            Kind::GetField { record, field } => {
                let Value::Record(name, fields) = read!(instruction.a) else {
                    exit!()
                };
                let Some(definition) = artifact.records.get(record as usize) else {
                    exit!()
                };
                let (Some(expected), Some(item)) = (
                    definition.fields.get(field as usize),
                    fields.get(field as usize),
                ) else {
                    exit!()
                };
                if name.as_str() != definition.name.as_str()
                    || !value_matches(item, &expected.ty, &artifact.records)
                {
                    exit!();
                }
                commit!(cloned_value_allocation(item));
                let value = clone_value_fast(item);
                drop_operands!();
                finish_value!(value)
            }
            Kind::MakeOk | Kind::MakeErr | Kind::MakeSome => {
                commit!(size_of::<Value>());
                let value = match instruction.a {
                    Src::Stack(_) => match stack.pop() {
                        Some(value) => value,
                        None => unreachable!("verified bytecode has an operand to wrap"),
                    },
                    Src::Local(slot) => clone_value_fast(&locals[slot as usize]),
                    Src::Const(index) => clone_value_fast(&program.consts[index as usize]),
                };
                let payload = Box::new(value);
                finish_value!(match instruction.kind {
                    Kind::MakeOk => Value::ResultOk(payload),
                    Kind::MakeErr => Value::ResultErr(payload),
                    _ => Value::OptionSome(payload),
                })
            }
            Kind::MakeRecord { record, count } => {
                let count = count as usize;
                let Some(definition) = artifact.records.get(record as usize) else {
                    exit!()
                };
                if count != definition.fields.len() || stack.len() < count {
                    exit!();
                }
                let record_bytes = size_of::<Vec<Value>>()
                    .saturating_add(count.saturating_mul(size_of::<Value>()))
                    .saturating_add(definition.name.len());
                commit!(record_bytes);
                let fields = stack.split_off(stack.len() - count);
                finish_value!(Value::Record(definition.name.clone(), Arc::new(fields)))
            }
            Kind::JsonGet => {
                let Some(child) = json_field(read!(instruction.a), read!(instruction.b), limits)
                else {
                    exit!()
                };
                let result = Value::ResultOk(Box::new(Value::Json(Arc::clone(child))));
                commit!(0);
                drop_operands!();
                finish_value!(result)
            }
            Kind::JsonGetInt => {
                let Some(field) = json_field(read!(instruction.a), read!(instruction.b), limits)
                    .and_then(|child| json_small_int(child))
                else {
                    exit!()
                };
                commit!(0);
                drop_operands!();
                finish_value!(Value::ResultOk(Box::new(field)))
            }
            Kind::JsonGetText => {
                let Some(text) = json_field(read!(instruction.a), read!(instruction.b), limits)
                    .and_then(|child| json_text(child, limits))
                else {
                    exit!()
                };
                commit!(size_of::<String>().saturating_add(text.len()));
                let text = SharedText::clone(text);
                drop_operands!();
                finish_value!(Value::ResultOk(Box::new(Value::Text(text))))
            }
            Kind::UnwrapJsonInt => {
                let Value::ResultOk(payload) = read!(instruction.a) else {
                    exit!()
                };
                let Value::Json(child) = payload.as_ref() else {
                    exit!()
                };
                let Some(field) = json_small_int(child) else {
                    exit!()
                };
                commit!(0);
                drop_operands!();
                finish_value!(Value::ResultOk(Box::new(field)))
            }
            Kind::UnwrapJsonText => {
                let Value::ResultOk(payload) = read!(instruction.a) else {
                    exit!()
                };
                let Value::Json(child) = payload.as_ref() else {
                    exit!()
                };
                let Some(text) = json_text(child, limits) else {
                    exit!()
                };
                commit!(size_of::<String>().saturating_add(text.len()));
                let text = SharedText::clone(text);
                drop_operands!();
                finish_value!(Value::ResultOk(Box::new(Value::Text(text))))
            }
            Kind::ParseInt => {
                let Some(parsed) = parse_short_digits(read!(instruction.a)) else {
                    exit!()
                };
                commit!(0);
                drop_operands!();
                finish_value!(Value::ResultOk(Box::new(parsed)))
            }
            Kind::IndexParseInt => {
                let (Value::List(items), Value::Int(position)) =
                    (read!(instruction.a), read!(instruction.b))
                else {
                    exit!()
                };
                let Some(item) = position
                    .small()
                    .and_then(|position| usize::try_from(position).ok())
                    .and_then(|position| items.get(position))
                else {
                    exit!()
                };
                let Some(parsed) = parse_short_digits(item) else {
                    exit!()
                };
                commit!(cloned_value_allocation_fast(item));
                drop_operands!();
                finish_value!(Value::ResultOk(Box::new(parsed)))
            }
            Kind::UnwrapGetField { record, field } => {
                let (Value::ResultOk(payload) | Value::OptionSome(payload)) = read!(instruction.a)
                else {
                    exit!()
                };
                let Value::Record(name, fields) = payload.as_ref() else {
                    exit!()
                };
                let Some(definition) = artifact.records.get(record as usize) else {
                    exit!()
                };
                let (Some(expected), Some(item)) = (
                    definition.fields.get(field as usize),
                    fields.get(field as usize),
                ) else {
                    exit!()
                };
                if name.as_str() != definition.name.as_str()
                    || !value_matches(item, &expected.ty, &artifact.records)
                {
                    exit!();
                }
                commit!(cloned_value_allocation(item));
                let value = clone_value_fast(item);
                drop_operands!();
                finish_value!(value)
            }
            Kind::Host {
                op_ip,
                check_result,
            } => {
                commit!(0);
                for source in &program.charges
                    [instruction.charges_start as usize..instruction.charges_end as usize]
                {
                    stack.push(match *source {
                        Src::Local(slot) => clone_value_fast(&locals[slot as usize]),
                        Src::Const(index) => clone_value_fast(&program.consts[index as usize]),
                        Src::Stack(_) => unreachable!("charges only name locals and constants"),
                    });
                }
                let operation = &function.code[op_ip as usize];
                host.host_op(HostOpContext {
                    instruction: operation,
                    function,
                    stack: &mut *stack,
                    locals: &mut *locals,
                    budget,
                    depth,
                })?;
                // A host operation can stop early on cancellation, leaving no result behind.
                if budget.is_cancelled() || host.externally_cancelled() {
                    return Ok(RegisterExit {
                        ip: op_ip as usize + 1,
                        poll_cancellation: true,
                    });
                }
                if check_result {
                    if let Some(result) = stack.last() {
                        if let Some(error) = value_limit_error(result, limits, operation.span) {
                            return Err(error);
                        }
                    }
                }
                pc += 1;
            }
            Kind::CallNone => {
                if depth.saturating_add(1) >= limits.call_depth_limit {
                    exit!();
                }
                commit!(0);
                finish_value!(Value::OptionNone)
            }
            Kind::CallSome => {
                if depth.saturating_add(1) >= limits.call_depth_limit
                    || fuel.remaining < u64::from(instruction.fuel)
                {
                    exit!();
                }
                let Some(value) = stack.pop() else { exit!() };
                let charged = budget.allocate(size_of::<Value>());
                let wrapped = Value::OptionSome(Box::new(value));
                let within_limits =
                    charged && value_limit_error(&wrapped, limits, Span::default()).is_none();
                if !within_limits {
                    if charged {
                        budget.refund_allocation(size_of::<Value>());
                    }
                    let Value::OptionSome(value) = wrapped else {
                        unreachable!("the wrapper was built just above")
                    };
                    stack.push(*value);
                    exit!();
                }
                fuel.remaining -= u64::from(instruction.fuel);
                finish_value!(wrapped)
            }
        }
    }
}

#[inline]
fn read_operand<'v>(
    program: &'v RegisterProgram,
    locals: &'v [Value],
    stack: &'v [Value],
    source: Src,
) -> Option<&'v Value> {
    match source {
        Src::Local(slot) => locals.get(slot as usize),
        Src::Const(index) => program.consts.get(index as usize),
        Src::Stack(distance) => stack
            .len()
            .checked_sub(distance as usize + 1)
            .and_then(|position| stack.get(position)),
    }
}

#[inline]
fn json_field<'v>(json: &'v Value, key: &Value, limits: &VmLimits) -> Option<&'v Arc<SharedJson>> {
    let (Value::Json(json), Value::Text(key)) = (json, key) else {
        return None;
    };
    if key.len() > limits.max_text_bytes {
        return None;
    }
    json.as_object()?.get(key.as_ref())
}

#[inline]
fn json_small_int(json: &SharedJson) -> Option<Value> {
    let number = json.as_number()?.as_i64()?;
    Some(Value::Int(BigInt::from_i64(number)))
}

#[inline]
fn json_text<'v>(json: &'v SharedJson, limits: &VmLimits) -> Option<&'v SharedText> {
    match json {
        SharedJson::String(text) if text.len() <= limits.max_text_bytes => Some(text),
        _ => None,
    }
}

/// Up to 18 unsigned digits always fit an `i64`. Everything else takes the host's parser.
#[inline]
fn parse_short_digits(value: &Value) -> Option<Value> {
    let Value::Text(text) = value else {
        return None;
    };
    if text.is_empty() || text.len() > 18 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parsed = text
        .bytes()
        .fold(0i64, |total, byte| total * 10 + i64::from(byte - b'0'));
    Some(Value::Int(BigInt::from_i64(parsed)))
}
