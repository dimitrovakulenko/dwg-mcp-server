use std::io::{self, BufReader};

use dwg_libredwg::LibreDwgFactory;
use dwg_worker_core::StdioHandler;

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = BufReader::new(stdin.lock());
    let writer = stdout.lock();

    let mut handler = StdioHandler::new(LibreDwgFactory);
    handler.serve(reader, writer)
}
