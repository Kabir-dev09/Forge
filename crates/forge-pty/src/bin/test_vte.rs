use vte::{Params, Parser, Perform};
struct Dummy;
impl Perform for Dummy {
    fn print(&mut self, _c: char) {}
    fn execute(&mut self, _byte: u8) {}
    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        if action == 'm' {
            let mut flat = Vec::new();
            for param in params.iter() {
                for sub in param.iter() {
                    flat.push(*sub);
                }
            }
            println!("flat: {:?}", flat);
        }
    }
}
fn main() {
    let mut parser = Parser::new();
    let mut dummy = Dummy;
    let seq = b"\x1b[38;2;1;4;32m";
    for &b in seq {
        parser.advance(&mut dummy, b);
    }
    let seq2 = b"\x1b[38:2::1:4:32m";
    for &b in seq2 {
        parser.advance(&mut dummy, b);
    }
}
