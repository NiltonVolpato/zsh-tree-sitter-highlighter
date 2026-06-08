use std::env;
use tests::{parse_spans, print_colorized, render_diagram, highlight_buffer};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: debug_zle <buffer>");
        std::process::exit(1);
    }
    let buffer = &args[1];

    let captured = highlight_buffer(buffer);

    let (clean_text, spans) = parse_spans(&captured);
    print_colorized(&clean_text, &spans);
    println!("\nDiagram:\n{}", render_diagram(&clean_text, &spans));
}
