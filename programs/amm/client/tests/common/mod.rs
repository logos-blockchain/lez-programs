use nssa_core::program::ProgramId;

pub fn program_id_hex(program_id: ProgramId) -> String {
    let mut output = String::with_capacity(64);
    for word in program_id {
        for byte in word.to_le_bytes() {
            output.push_str(&format!("{byte:02x}"));
        }
    }
    output
}
