# frozen_string_literal: true

desc 'Validate .desc file without generating'
long_desc <<~DESC
  Parses and validates the .desc specification file, checking for:
  - Syntax errors
  - Undefined type references
  - Undefined function references
  - Empty states
  - Other semantic issues

  Examples:
    descent validate parser.desc
DESC

required_arg :file, desc: '.desc specification file'

def run
  require 'descent'

  unless File.exist?(file)
    Output.error "File not found: #{file}"
    exit 1
  end

  begin
    content = File.read(file)
    tokens  = Descent::Lexer.new(content, source_file: file).tokenize
    ast     = Descent::Parser.new(tokens).parse
    ir      = Descent::IRBuilder.new(ast).build

    result  = Descent::Validator.new(ir).validate

    if result.errors.any?
      Output.error "Validation failed with #{result.errors.size} error(s):"
      result.errors.each { |e| Output.bullet e }
      exit 1
    end

    if result.warnings.any?
      Output.warn "Validation passed with #{result.warnings.size} warning(s):"
      result.warnings.each { |w| Output.bullet w }
    else
      Output.success "Validation passed: #{file}"
    end

    # Show summary
    puts
    puts "Types:     #{ir.types.size}"
    puts "Functions: #{ir.functions.size}"
    puts "States:    #{ir.functions.sum { |f| f.states.size }}"
  rescue Descent::Error => e
    Output.error e.message
    exit 1
  end
end
