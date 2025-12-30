use descent_harness::Parser;
use std::io::Read;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--bench" {
        // Benchmark mode
        let mut input = Vec::new();
        std::io::stdin().read_to_end(&mut input).unwrap();
        let size_mb = input.len() as f64 / 1024.0 / 1024.0;
        
        let mut count = 0usize;
        let iters = 10;
        let start = Instant::now();
        for _ in 0..iters {
            let mut c = 0usize;
            Parser::new(&input).parse(|_| c += 1);
            count = c;
        }
        let elapsed = start.elapsed().as_secs_f64();
        let per_iter = elapsed / iters as f64;
        let throughput = size_mb / per_iter;
        
        eprintln!("{:.2} MB, {} events, {:.3}s/iter, {:.1} MB/s", 
                  size_mb, count, per_iter, throughput);
    } else {
        // Normal mode - print events
        let mut input = Vec::new();
        std::io::stdin().read_to_end(&mut input).unwrap();
        Parser::new(&input).parse(|event| {
            println!("{}", event.format_line());
        });
    }
}
