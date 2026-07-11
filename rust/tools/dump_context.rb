# frozen_string_literal: true

# Dump Ruby descent's generator context (Generator#build_context) as JSON for
# differential testing against `descent-rs context`. This is the IR-context
# checkpoint from rust/PROGRESS.md: the full template input, including the
# ir_builder's call-arg transform and the generator's local-init/mutability/
# helper-usage analysis.
#
# NOTE: build_context is private by design; this tool reaches in with #send
# on purpose (differential instrument only, Ruby side untouched).
#
# Usage: ruby -I lib rust/tools/dump_context.rb FILE.desc [trace]

require 'descent'
require 'json'

path = ARGV[0] or abort 'usage: dump_context.rb FILE.desc [trace]'
trace = ARGV[1] == 'true'

content = File.read(path)
tokens  = Descent::Lexer.new(content, source_file: path).tokenize
ast     = Descent::Parser.new(tokens).parse
ir      = Descent::IRBuilder.new(ast).build
gen     = Descent::Generator.new(ir, target: :rust, trace: trace)

puts JSON.pretty_generate(gen.send(:build_context))
