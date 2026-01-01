# frozen_string_literal: true

desc 'Generate railroad diagram from .desc file'
long_desc <<~DESC
  Generates Python code that produces railroad diagrams (like JSON.org).
  The output uses the railroad-diagrams library.

  Examples:
    descent diagram parser.desc > diagram.py
    python diagram.py > diagram.html       # All diagrams as HTML
    python diagram.py --split              # Each function as separate SVG

  Requires: pip install railroad-diagrams
DESC

required_arg :file, desc: '.desc specification file'
flag :output, '-o FILE', '--output=FILE', desc: 'Output file (default: stdout)'

def run
  require 'descent'
  require 'descent/railroad'

  unless File.exist?(file)
    Output.error "File not found: #{file}"
    exit 1
  end

  begin
    content     = File.read(file)
    source_file = file

    tokens = Descent::Lexer.new(content, source_file:).tokenize
    ast    = Descent::Parser.new(tokens).parse
    ir     = Descent::IRBuilder.new(ast).build

    code = Descent::Railroad.new(ir).generate

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
