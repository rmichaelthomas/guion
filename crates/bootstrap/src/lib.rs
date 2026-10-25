mod bytecode_ir;

pub fn assemble_ir_file(input: &str, output: &str) -> Result<(), String> {
    bytecode_ir::assemble_file(input, output)
}
