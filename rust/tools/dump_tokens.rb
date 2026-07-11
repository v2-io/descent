# frozen_string_literal: true

# Dump Ruby descent's lexer Tokens as canonical JSON for differential testing
# against `descent-rs tokens`. Shape must stay in lockstep with
# rust/libdescent/src/dump.rs.
#
# Usage: ruby -I lib rust/tools/dump_tokens.rb FILE.desc

require 'descent'
require 'json'

path = ARGV[0] or abort 'usage: dump_tokens.rb FILE.desc'
tokens = Descent::Lexer.new(File.read(path), source_file: path).tokenize
puts JSON.pretty_generate(tokens.map { |t|
  { tag: t.tag, id: t.id, rest: t.rest, lineno: t.lineno }
})
