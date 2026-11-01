// Throwaway developer tool: print the opcode stream of one function in a .gub artifact.
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).expect("read artifact");
    let artifact = guion_model::Artifact::from_bytes(&bytes).expect("decode artifact");
    for function in &artifact.functions {
        if function.name == args[2] {
            for (ip, instruction) in function.code.iter().enumerate() {
                println!("{ip:4} {:?}", instruction.op);
            }
        }
    }
}
