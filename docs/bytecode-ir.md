# Veritas bytecode IR interchange

`veritas assemble-ir <input.vri.json> --output <program.vrb>` is the stage-zero artifact writer interface for a compiler implemented in Veritas. The producer owns source loading, lexing, parsing, semantic analysis, and bytecode lowering. The Rust command validates the interchange object and serializes it using the current portable artifact format; it is not a source compiler and does not infer types or repair invalid control flow.

The JSON document has schema `veritas-bytecode-ir/v1`:

```json
{
  "schema": "veritas-bytecode-ir/v1",
  "source_name": "src/main.vr",
  "entry": "main",
  "functions": [
    {
      "name": "main",
      "parameters": [],
      "result": "Int",
      "local_count": 0,
      "code": [
        {"op": "ConstInt", "value": "42"},
        {"op": "Return"}
      ]
    }
  ]
}
```

`source_name` defaults to `generated.vr`. `parameters` is an array of type strings, and `local_count` includes parameter slots. Types are `Int`, `Float64`, `Bool`, `Text`, `Bytes`, `Json`, `JsonError`, `FileError`, `HttpError`, `TaskError`, `Unit`, `Record[Name]`, `List[T]`, `Map[T]`, `Option[T]`, and `Result[T, E]`. Nested generic types are allowed. Optional `records` contains `{ "name": "Point", "fields": [{"name":"x","type":"Int"}] }` definitions. Optional `externals` contains external-call declarations with `name`, `module`, `callable`, `parameter_names`, `parameters`, and `result` fields.

Each instruction has an `op` name and opcode-specific fields. Optional `span` is `{ "source_id": 0, "start": 0, "end": 0, "line": 1, "column": 1 }`; omitted spans use the instruction index for the column. `ConstInt.value` is decimal text to preserve arbitrary precision, `ConstFloat64.value` is finite decimal text, `ConstBool.value` is JSON boolean, and `ConstText.value` is a JSON string. The assembler currently accepts the bytecode operations implemented in `crates/cli/src/bytecode_ir.rs`; unsupported names fail closed.

Control-flow targets may be numeric instruction offsets in the current function or symbolic labels. A `{ "op": "Label", "id": 10 }` marker declares a label at that point and is removed during assembly; `Jump` and `JumpIfFalse` can use `label_id` instead of `target`. Duplicate and unresolved labels fail before an artifact is written. `Call.function` names another function in the same `functions` array and carries `argc`. `PythonCall.function` and `PythonCallKw.function` name an entry in `externals`; keyword calls also carry `keyword_names`. `MakeRecord.record` names a record and carries a field-value `count`; `GetField` carries a record name and field name. `Load.local`, `Store.local`, and local-mutating append operations carry a zero-based local slot. Collection construction/update operations carry `count`, `item_type`, or `local` as shown by the operation. All instruction references, function signatures, and artifact invariants are checked by the artifact model before bytes are written.

This interchange is an internal bootstrap boundary, not a stable public bytecode API. Its schema version and artifact format version are distinct: the former describes the compiler-to-assembler JSON, while the latter is embedded in the produced `.vrb`.
