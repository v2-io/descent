# frozen_string_literal: true

desc 'Generate parser from .desc file'
long_desc <<~DESC
  Parses the .desc specification file and generates a parser in the
  target language. Currently supports Rust output.

  Examples:
    descent generate parser.desc
    descent generate parser.desc -o src/parser.rs
    descent generate parser.desc --target rust
    descent generate parser.desc --trace
DESC

required_arg :file, desc: '.desc specification file'
flag :output, '-o FILE', '--output=FILE', desc: 'Output file (default: stdout)'
flag :target, '-t TARGET', '--target=TARGET', desc: 'Target language: rust, c (default: rust)'
flag :trace, '--trace', desc: 'Enable trace output in generated parser'

def run
  require 'descent'

  unless File.exist?(file)
    Output.error "File not found: #{file}"
    exit 1
  end

  target_sym = (target || 'rust').to_sym
  trace_opt  = trace ? true : false

  begin
    code = Descent.generate(file, target: target_sym, trace: trace_opt)

    # Clean up excessive whitespace
    code = code.gsub(/\n{3,}/, "\n\n")

    if output
      File.write(output, code)
      Output.success "Generated #{output}"
    else
      puts code
    end
  rescue Descent::Error => e
    Output.error e.message
    exit 1
  end
end
